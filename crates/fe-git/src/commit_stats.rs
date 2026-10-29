//! Each file's added and removed line counts for a commit, from `git show --numstat`.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::cli::{self, Access};
use crate::commit_log::path_of;
use crate::{GitError, Repo};

/// One changed file's line counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: PathBuf,
    /// The old path of a rename.
    pub from: Option<PathBuf>,
    /// Lines added; `None` for a binary file.
    pub added: Option<u32>,
    /// Lines removed; `None` for a binary file.
    pub removed: Option<u32>,
}

/// The files a commit changed with their line counts, sorted by path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitStats {
    pub files: Vec<FileStat>,
}

impl CommitStats {
    /// Lines added and removed over all files; binary files count as none.
    pub fn total(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(added, removed), file| {
            (
                added.saturating_add(file.added.unwrap_or(0)),
                removed.saturating_add(file.removed.unwrap_or(0)),
            )
        })
    }
}

impl Repo {
    /// The line counts of `commit` (an id or any revision) against its first
    /// parent, or the empty tree for a root commit, renames detected. Runs one
    /// `git show`; blocks until it exits.
    pub fn commit_stats(&self, commit: &str) -> Result<CommitStats, GitError> {
        let mut args: Vec<OsString> = [
            "show",
            "--numstat",
            "-z",
            "--format=",
            "-M",
            "-m",
            "--first-parent",
            "--root",
            "--no-relative",
            "--end-of-options",
        ]
        .map(OsString::from)
        .into();
        args.push(commit.into());
        let stdout = cli::run(&self.workdir, Access::Read, &args, None)?;

        let mut files = Vec::new();
        let mut tokens = stdout.split(|&byte| byte == 0);
        while let Some(record) = tokens.next() {
            let record = record.strip_prefix(b"\n").unwrap_or(record);
            if record.is_empty() {
                continue;
            }
            let mut fields = record.splitn(3, |&byte| byte == b'\t');
            let (Some(added), Some(removed), Some(path)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            // A rename has an empty path here, then the old and new paths.
            let (from, path) = if path.is_empty() {
                let (Some(from), Some(to)) = (tokens.next(), tokens.next()) else {
                    break;
                };
                (Some(path_of(from)), path_of(to))
            } else {
                (None, path_of(path))
            };
            files.push(FileStat {
                path: self.workdir.join(path),
                from: from.map(|from| self.workdir.join(from)),
                added: count(added),
                removed: count(removed),
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(CommitStats { files })
    }
}

/// A numstat count; `-` (binary) is `None`.
fn count(field: &[u8]) -> Option<u32> {
    std::str::from_utf8(field).ok()?.parse().ok()
}
