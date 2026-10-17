//! The commits that changed a file, newest first, following renames.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gix::ObjectId;

use crate::blame::{commit_info, git_path};
use crate::cli::{self, Access};
use crate::{CommitInfo, GitError, Repo};

/// One commit that changed a file, as `git log --follow` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCommit {
    pub commit: Arc<CommitInfo>,
    /// Absolute: the file's path in this commit (for a commit that deleted it,
    /// the path it had).
    pub path: PathBuf,
    /// Absolute: its path in the first parent, when this commit renamed or
    /// copied it there.
    pub from: Option<PathBuf>,
}

impl Repo {
    /// The commits that changed `path` (absolute), newest first, following
    /// renames, at most `limit`. Empty for an untracked file, an unborn HEAD or
    /// a `limit` of 0. Like `git log -- <file>`, lists a merge that resolved
    /// the file differently from every parent. A merge after a rename in one
    /// parent is still left out: its combined diff is empty under `--follow`.
    /// Runs one `git log`; blocks until it exits.
    pub fn file_log(&self, path: &Path, limit: usize) -> Result<Vec<FileCommit>, GitError> {
        let rela_path = self
            .rela_path(path)
            .ok_or_else(|| GitError::OutsideWorktree(path.to_path_buf()))?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let git_path = git_path(rela_path);
        // Explicit flags, so neither the user's nor the repo's config (such as
        // `log.showRoot`) changes the output.
        let mut args: Vec<OsString> = [
            "-c",
            "log.follow=false",
            "-c",
            "diff.renames=true",
            "log",
            "--follow",
            "--cc",
            "--root",
            "-z",
            "--name-status",
            "--no-color",
            "--no-decorate",
            "--no-show-signature",
            "--no-abbrev",
            "--format=%H",
        ]
        .map(OsString::from)
        .into();
        args.push(format!("--max-count={limit}").into());
        args.push("--".into());
        #[cfg(unix)]
        args.push({
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(&git_path).to_owned()
        });
        #[cfg(not(unix))]
        args.push(git_path.to_string().into());
        let stdout = match cli::run(&self.workdir, Access::Read, &args, None) {
            Ok(stdout) => stdout,
            Err(GitError::Command { stderr, .. })
                if stderr.contains("does not have any commits") =>
            {
                return Ok(Vec::new());
            }
            Err(err) => return Err(err),
        };

        let repo = self.repo.to_thread_local();
        let mut infos: HashMap<String, Arc<CommitInfo>> = HashMap::new();
        let mut commits = Vec::new();
        for entry in parse(&stdout) {
            let commit = match infos.get(&entry.id) {
                Some(info) => info.clone(),
                None => {
                    let id = ObjectId::from_hex(entry.id.as_bytes()).map_err(GitError::from_gix)?;
                    let info = Arc::new(commit_info(&repo, id)?);
                    infos.insert(entry.id, info.clone());
                    info
                }
            };
            commits.push(FileCommit {
                commit,
                path: self.workdir.join(entry.path),
                from: entry.from.map(|from| self.workdir.join(from)),
            });
        }
        Ok(commits)
    }
}

struct Entry {
    id: String,
    path: PathBuf,
    from: Option<PathBuf>,
}

/// Parses `git log -z --cc --name-status --format=%H`: per commit its id, then
/// `\n<status>`, then one path (two for a rename or copy: old, new), each
/// NUL-terminated. A merge that changed the file has an empty token after its
/// id, then one status letter per parent (`MM`) and one path; a merge that
/// listed nothing has the empty token and the next id follows.
fn parse(stdout: &[u8]) -> Vec<Entry> {
    let mut tokens = stdout.split(|&byte| byte == 0).peekable();
    let mut entries = Vec::new();
    while let Some(id) = tokens.next() {
        if id.is_empty() {
            continue;
        }
        let id = String::from_utf8_lossy(id).trim().to_owned();
        if tokens.next_if(|token| token.is_empty()).is_some() {
            let combined = tokens.next_if(|token| {
                !token.is_empty() && token.iter().all(|byte| byte.is_ascii_uppercase())
            });
            if combined.is_none() {
                continue;
            }
            let Some(path) = tokens.next() else { break };
            entries.push(Entry {
                id,
                path: path_of(path),
                from: None,
            });
            continue;
        }
        let Some(status) = tokens.next_if(|token| token.starts_with(b"\n")) else {
            continue;
        };
        let two_paths = matches!(status.get(1), Some(b'R' | b'C'));
        let Some(first) = tokens.next() else { break };
        let (from, path) = if two_paths {
            let Some(second) = tokens.next() else { break };
            (Some(path_of(first)), path_of(second))
        } else {
            (None, path_of(first))
        };
        entries.push(Entry { id, path, from });
    }
    entries
}

fn path_of(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}
