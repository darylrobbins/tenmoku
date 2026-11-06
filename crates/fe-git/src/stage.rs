//! Staging and discarding, through the `git` executable (see [`crate::cli`]).

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::cli::{Access, run};
use crate::{GitError, Repo};

fn os(arg: &str) -> OsString {
    OsString::from(arg)
}

/// What exec charges for one argument or environment entry: its bytes, a NUL
/// and the pointer to it.
fn exec_cost(arg: &OsStr) -> usize {
    arg.len() + 1 + std::mem::size_of::<usize>()
}

const FROM_STDIN: &str = "--pathspec-from-file=-";
const NUL: &str = "--pathspec-file-nul";

/// `names` as NUL-terminated entries, the stdin form of `--pathspec-from-file`,
/// so the list never counts against the argument limit.
fn nul_joined(names: &[OsString]) -> Result<Vec<u8>, GitError> {
    let mut out = Vec::new();
    for name in names {
        let bytes = gix::path::os_str_into_bstr(name)
            .map_err(|_| GitError::Io(format!("{} is not a valid path", name.to_string_lossy())))?;
        if bytes.contains(&0) {
            return Err(GitError::Io(format!(
                "{} is not a valid path",
                name.to_string_lossy()
            )));
        }
        out.extend_from_slice(bytes.as_ref());
        out.push(0);
    }
    Ok(out)
}

impl Repo {
    /// Stages `paths`: additions, modifications and deletions alike, as VS
    /// Code's Stage Changes does.
    pub fn stage(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let args = [os("add"), os("-A"), os(FROM_STDIN), os(NUL)];
        run(
            &self.workdir,
            Access::Write,
            &args,
            Some(&nul_joined(&relas)?),
        )
        .map(drop)
    }

    /// Takes `paths` out of the index again, back to HEAD. Works while HEAD is
    /// unborn, where that means dropping them from the index.
    pub fn unstage(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let args = if self.head_is_unborn()? {
            // `-f`: the entry may match neither HEAD nor the working tree.
            vec![
                os("rm"),
                os("--cached"),
                os("-f"),
                os("-q"),
                os("-r"),
                os("--ignore-unmatch"),
            ]
        } else {
            vec![os("reset"), os("-q")]
        };
        let args = [args, vec![os(FROM_STDIN), os(NUL)]].concat();
        run(
            &self.workdir,
            Access::Write,
            &args,
            Some(&nul_joined(&relas)?),
        )
        .map(drop)
    }

    /// Throws away the working-tree changes to `paths`: a tracked file goes back
    /// to its index text, an untracked one is deleted from disk.
    pub fn discard(&self, paths: &[PathBuf]) -> Result<(), GitError> {
        let Some(relas) = self.relative(paths)? else {
            return Ok(());
        };
        let tracked = self.tracked_names(&relas)?;

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
            let args = [os("checkout"), os("-q"), os(FROM_STDIN), os(NUL)];
            run(
                &self.workdir,
                Access::Write,
                &args,
                Some(&nul_joined(&restore)?),
            )?;
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

    /// The names among `relas` that the index tracks at any stage, sparse
    /// directory entries expanded, asked of `ls-files` in batches that fit the
    /// argument space. Each path, fixed argument and inherited environment
    /// entry is charged its bytes, a NUL and a pointer (see [`exec_cost`]),
    /// against half of [`ARG_SPACE`]; a batch takes at least one path.
    fn tracked_names(&self, relas: &[OsString]) -> Result<HashSet<Vec<u8>>, GitError> {
        const ARG_SPACE: usize = 128 * 1024;
        let fixed = [
            os("git"),
            os("--literal-pathspecs"),
            os("-C"),
            self.workdir.clone().into_os_string(),
            os("ls-files"),
            os("-z"),
            os("--"),
        ];
        // The three variables `cli::run_raw` sets on every git it spawns.
        let added_env = [
            os("GIT_TERMINAL_PROMPT=0"),
            os("LC_ALL=C"),
            os("GIT_OPTIONAL_LOCKS=0"),
        ];
        let inherited: usize = std::env::vars_os()
            .map(|(key, value)| key.len() + value.len() + 2 + std::mem::size_of::<usize>())
            .sum();
        let overhead = inherited
            + fixed
                .iter()
                .chain(&added_env)
                .map(|arg| exec_cost(arg))
                .sum::<usize>();
        let budget = (ARG_SPACE / 2).saturating_sub(overhead);
        let mut tracked = HashSet::new();
        let mut start = 0;
        while start < relas.len() {
            let mut end = start;
            let mut size = 0;
            while end < relas.len() && (end == start || size + exec_cost(&relas[end]) <= budget) {
                size += exec_cost(&relas[end]);
                end += 1;
            }
            let mut args = vec![os("ls-files"), os("-z"), os("--")];
            args.extend(relas[start..end].iter().cloned());
            let listing = run(&self.workdir, Access::Read, &args, None)?;
            tracked.extend(
                listing
                    .split(|&byte| byte == 0)
                    .filter(|name| !name.is_empty())
                    .map(<[u8]>::to_vec),
            );
            start = end;
        }
        Ok(tracked)
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
        let root = dunce::canonicalize(&self.workdir).map_err(|_| outside())?;
        let repo = self.repo.to_thread_local();
        let metadata: Vec<PathBuf> = [
            repo.git_dir(),
            repo.common_dir(),
            &self.workdir.join(".git"),
        ]
        .into_iter()
        .filter_map(|dir| dunce::canonicalize(dir).ok())
        .collect();
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if let Ok(real) = dunce::canonicalize(dir) {
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
        let name = rela.as_os_str();
        // git precomposes the paths on its command line, but not the ones read
        // from `--pathspec-from-file`, so do it here for the index's NFC names.
        if cfg!(target_os = "macos") && self.precompose_unicode() {
            return Ok(gix::utils::str::precompose_os_string(name.into()).into_owned());
        }
        Ok(name.to_owned())
    }

    /// `core.precomposeUnicode`, false when unset as git reads it.
    fn precompose_unicode(&self) -> bool {
        self.repo
            .to_thread_local()
            .config_snapshot()
            .boolean("core.precomposeUnicode")
            .unwrap_or(false)
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
