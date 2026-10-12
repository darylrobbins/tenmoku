//! Staging and discarding, through the `git` executable (see [`crate::cli`]).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::{Access, run};
use crate::{GitError, Repo};

fn os(arg: &str) -> OsString {
    OsString::from(arg)
}

impl Repo {
    /// Stages `paths`: additions, modifications and deletions alike, as VS
    /// Code's Stage Changes does.
    pub fn stage(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let mut args = vec![os("add"), os("-A"), os("--")];
        args.extend(relas);
        run(&self.workdir, Access::Write, &args, None).map(drop)
    }

    /// Takes `paths` out of the index again, back to HEAD. Works while HEAD is
    /// unborn, where that means dropping them from the index.
    pub fn unstage(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let mut args = if self.head_is_unborn()? {
            // `-f`: the entry may match neither HEAD nor the working tree.
            vec![
                os("rm"),
                os("--cached"),
                os("-f"),
                os("-q"),
                os("-r"),
                os("--ignore-unmatch"),
                os("--"),
            ]
        } else {
            vec![os("reset"), os("-q"), os("--")]
        };
        args.extend(relas);
        run(&self.workdir, Access::Write, &args, None).map(drop)
    }

    /// Throws away the working-tree changes to `paths`: a tracked file goes back
    /// to its index text, an untracked one is deleted from disk.
    pub fn discard(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let mut args = vec![os("ls-files"), os("-z"), os("--")];
        args.extend(relas.iter().cloned());
        let listing = run(&self.workdir, Access::Read, &args, None)?;
        let tracked: HashSet<&[u8]> = listing
            .split(|&byte| byte == 0)
            .filter(|name| !name.is_empty())
            .collect();

        let mut restore = Vec::new();
        let mut delete = Vec::new();
        for (path, rela) in paths.iter().zip(relas) {
            let name = gix::path::os_str_into_bstr(&rela).ok();
            if name.is_some_and(|name| tracked.contains::<[u8]>(name.as_ref())) {
                restore.push(rela);
            } else {
                delete.push(path);
            }
        }
        if !restore.is_empty() {
            let mut args = vec![os("checkout"), os("-q"), os("--")];
            args.extend(restore);
            run(&self.workdir, Access::Write, &args, None)?;
        }
        for path in delete {
            self.check_deletable(path)?;
            let removed = match path.symlink_metadata() {
                Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
                Ok(_) => std::fs::remove_file(path),
                Err(err) => Err(err),
            };
            match removed {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return Err(GitError::Io(err.to_string()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Makes `text` the index's content for `path` without touching the working
    /// tree, the way VS Code stages a hunk. The text goes through the clean
    /// filters and autocrlf like `git add` would. An existing entry keeps its
    /// mode; a new one is `100644`, or `100755` for an executable file on disk.
    pub fn set_index_text(&self, path: &Path, text: &str) -> Result<(), GitError> {
        let rela = self.relative_one(path)?;
        let hash_args = [
            os("hash-object"),
            os("-w"),
            os("--stdin"),
            os("--path"),
            rela.clone(),
        ];
        let id = run(
            &self.workdir,
            Access::Write,
            &hash_args,
            Some(text.as_bytes()),
        )?;
        let id = String::from_utf8_lossy(&id).trim().to_owned();

        let mut cacheinfo = OsString::from(format!("{},{id},", self.index_mode(path, &rela)));
        cacheinfo.push(&rela);
        let args = [
            os("update-index"),
            os("--add"),
            os("--cacheinfo"),
            cacheinfo,
        ];
        run(&self.workdir, Access::Write, &args, None).map(drop)
    }

    /// Whether HEAD is unborn; a HEAD that can't be read is an error.
    fn head_is_unborn(&self) -> Result<bool, GitError> {
        let repo = self.repo.to_thread_local();
        let mut head = repo.head().map_err(GitError::from_gix)?;
        if head.is_unborn() {
            return Ok(true);
        }
        head.peel_to_commit().map_err(GitError::from_gix)?;
        Ok(false)
    }

    /// An untracked `path` may be deleted when it's a file or symlink (not a
    /// directory, which may hold tracked files) and no symlink on the way leads
    /// out of the working tree.
    fn check_deletable(&self, path: &Path) -> Result<(), GitError> {
        let outside = || GitError::OutsideWorktree(path.to_path_buf());
        if path.symlink_metadata().is_ok_and(|meta| meta.is_dir()) {
            return Err(GitError::Io(format!(
                "{} is a directory; discard its files",
                path.display()
            )));
        }
        let root = self.workdir.canonicalize().map_err(|_| outside())?;
        let repo = self.repo.to_thread_local();
        let metadata: Vec<PathBuf> = [
            repo.git_dir(),
            repo.common_dir(),
            &self.workdir.join(".git"),
        ]
        .into_iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect();
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if let Ok(real) = dir.canonicalize() {
                let in_metadata = metadata.iter().any(|dir| real.starts_with(dir));
                return if real.starts_with(&root) && !in_metadata {
                    Ok(())
                } else {
                    Err(outside())
                };
            }
            parent = dir.parent();
        }
        Err(outside())
    }

    /// The mode for `path`'s index entry: the entry's own, else by the file on disk.
    fn index_mode(&self, path: &Path, rela: &OsString) -> &'static str {
        use gix::index::entry::Mode;
        let existing = gix::path::os_str_into_bstr(rela).ok().and_then(|name| {
            let repo = self.repo.to_thread_local();
            let index = repo.index_or_empty().ok()?;
            let entry =
                index.entry_by_path_and_stage(name, gix::index::entry::Stage::Unconflicted)?;
            Some(entry.mode)
        });
        match existing {
            Some(Mode::FILE_EXECUTABLE) => "100755",
            Some(Mode::SYMLINK) => "120000",
            Some(_) => "100644",
            None if is_executable(path) => "100755",
            None => "100644",
        }
    }

    /// `paths` relative to the working tree; `None` for an empty list.
    fn relative(&self, paths: &[PathBuf]) -> Result<Option<Vec<OsString>>, GitError> {
        if paths.is_empty() {
            return Ok(None);
        }
        paths
            .iter()
            .map(|path| self.relative_one(path))
            .collect::<Result<_, _>>()
            .map(Some)
    }

    fn relative_one(&self, path: &Path) -> Result<OsString, GitError> {
        use std::path::Component;
        let outside = || GitError::OutsideWorktree(path.to_path_buf());
        let rela = path.strip_prefix(&self.workdir).map_err(|_| outside())?;
        let plain = rela
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        // Repository metadata is never a file to stage or discard.
        let metadata = rela.components().next().is_some_and(|first| {
            first
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
        });
        if rela.as_os_str().is_empty() || !plain || metadata {
            return Err(outside());
        }
        Ok(rela.as_os_str().to_owned())
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}
