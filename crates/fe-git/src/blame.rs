//! Per-row blame of an editor's text, and a file's text at any commit, for
//! the blame gutter and for opening the change a commit made to a file.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

use crate::{GitError, Repo, commit_blob, commit_text, is_binary};

/// The path as git names it: the normalized components (no `.` or doubled
/// separators) joined with `/`, each keeping its exact bytes.
pub(crate) fn git_path(rela_path: &Path) -> BString {
    let mut out = BString::default();
    for (i, part) in rela_path.components().enumerate() {
        if i > 0 {
            out.push(b'/');
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            out.extend_from_slice(part.as_os_str().as_bytes());
        }
        #[cfg(not(unix))]
        out.extend_from_slice(part.as_os_str().to_string_lossy().as_bytes());
    }
    out
}

/// A blob or text over this many bytes isn't blamed.
const MAX_BLAME_BYTES: usize = 5 * 1024 * 1024;

/// A run of rows of the blamed text last changed by one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameRun {
    /// Rows of the blamed text. The empty row after a final line break is in
    /// no run.
    pub rows: Range<u32>,
    /// The commit that last changed these rows, `None` for rows not committed
    /// yet. Runs of one commit share its `Arc`.
    pub commit: Option<Arc<CommitInfo>>,
}

/// What blame shows of a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// The full id in hex.
    pub id: String,
    /// The id's first 7 hex digits.
    pub short: String,
    pub author: String,
    pub email: String,
    /// Author time, seconds since the epoch.
    pub time: i64,
    /// The author time's offset from UTC, in minutes.
    pub offset_minutes: i32,
    /// The message's first line with text, as `git blame` gives it.
    pub summary: String,
    /// The whole message.
    pub message: String,
}

impl Repo {
    /// Blames each row of `text`, the editor's current text of `path`: HEAD's
    /// blob is blamed from HEAD, then its rows are matched to `text`'s with a
    /// row diff. Rows the diff keeps have the commit that last changed them;
    /// edited and added rows have none. A file not in HEAD (or HEAD unborn) is
    /// one uncommitted run. Empty for a binary blob or text, or one over 5 MiB.
    /// Rows end at `\n`; a `\r` before it is ignored.
    pub fn blame(&self, path: &Path, text: &str) -> Result<Vec<BlameRun>, GitError> {
        let rela_path = self
            .rela_path(path)
            .ok_or_else(|| GitError::OutsideWorktree(path.to_path_buf()))?;
        if text.len() > MAX_BLAME_BYTES || is_binary(text.as_bytes()) {
            return Ok(Vec::new());
        }
        let text_rows = rows(text.as_bytes());
        let uncommitted = || {
            let rows = text_rows.0.len() as u32;
            let run = BlameRun {
                rows: 0..rows,
                commit: None,
            };
            Ok(if rows == 0 { Vec::new() } else { vec![run] })
        };

        let repo = self.repo.to_thread_local();
        let mut head = repo.head().map_err(GitError::from_gix)?;
        if head.is_unborn() {
            return uncommitted();
        }
        let head = head.peel_to_commit().map_err(GitError::from_gix)?;
        let Some(blob) = commit_blob(&head, rela_path)? else {
            return uncommitted();
        };
        if blob.data.len() > MAX_BLAME_BYTES || is_binary(&blob.data) {
            return Ok(Vec::new());
        }
        let head_rows = rows(&blob.data);

        let git_path = git_path(rela_path);
        // As `git blame` does, follow a file across a rename.
        let options = gix::repository::blame_file::Options {
            rewrites: Some(gix::diff::Rewrites::default()),
            ..Default::default()
        };
        let outcome = repo
            .blame_file(git_path.as_bstr(), head.id, options)
            .map_err(GitError::from_gix)?;
        let mut head_commits: Vec<Option<ObjectId>> = vec![None; head_rows.0.len()];
        for entry in &outcome.entries {
            let start = entry.start_in_blamed_file as usize;
            let end = (start + entry.len.get() as usize).min(head_commits.len());
            for commit in head_commits.get_mut(start..end).into_iter().flatten() {
                *commit = Some(entry.commit_id);
            }
        }

        // Each row of `text` takes the commit of the HEAD row the diff matches
        // it to; rows in a hunk are uncommitted.
        let mut text_commits: Vec<Option<ObjectId>> = Vec::with_capacity(text_rows.0.len());
        let input = InternedInput::new(head_rows, text_rows);
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        let mut head_row = 0;
        for hunk in diff.hunks() {
            text_commits.extend(&head_commits[head_row as usize..hunk.before.start as usize]);
            text_commits.extend(hunk.after.map(|_| None));
            head_row = hunk.before.end;
        }
        text_commits.extend(head_commits[head_row as usize..].iter().copied());
        debug_assert_eq!(text_commits.len(), input.after.len());

        let mut infos: HashMap<ObjectId, Arc<CommitInfo>> = HashMap::new();
        let mut runs: Vec<BlameRun> = Vec::new();
        let mut row = 0u32;
        for chunk in text_commits.chunk_by(|a, b| a == b) {
            let end = row + chunk.len() as u32;
            let commit = match chunk[0] {
                Some(id) => Some(match infos.get(&id) {
                    Some(info) => info.clone(),
                    None => {
                        let info = Arc::new(commit_info(&repo, id)?);
                        infos.insert(id, info.clone());
                        info
                    }
                }),
                None => None,
            };
            runs.push(BlameRun {
                rows: row..end,
                commit,
            });
            row = end;
        }
        Ok(runs)
    }

    /// The text of `path` at `commit` (a full hex id, as [`CommitInfo::id`]
    /// and [`Repo::parent`] give), decoded as UTF-8 (lossily). `None` when the commit's tree has
    /// no file at `path`, `path` is outside the working tree, or the blob is
    /// binary.
    pub fn text_at(&self, commit: &str, path: &Path) -> Result<Option<String>, GitError> {
        let Some(rela_path) = self.rela_path(path) else {
            return Ok(None);
        };
        let repo = self.repo.to_thread_local();
        let commit = resolve_commit(&repo, commit)?;
        commit_text(&commit, rela_path)
    }

    /// The full id of `commit`'s first parent, `None` for a root commit.
    pub fn parent(&self, commit: &str) -> Result<Option<String>, GitError> {
        let repo = self.repo.to_thread_local();
        let commit = resolve_commit(&repo, commit)?;
        Ok(commit.parent_ids().next().map(|id| id.to_string()))
    }
}

/// The commit with the full hex id `commit`.
fn resolve_commit<'repo>(
    repo: &'repo gix::Repository,
    commit: &str,
) -> Result<gix::Commit<'repo>, GitError> {
    let id = ObjectId::from_hex(commit.as_bytes()).map_err(GitError::from_gix)?;
    repo.find_commit(id).map_err(GitError::from_gix)
}

pub(crate) fn commit_info(repo: &gix::Repository, id: ObjectId) -> Result<CommitInfo, GitError> {
    let commit = repo.find_commit(id).map_err(GitError::from_gix)?;
    let author = commit.author().map_err(GitError::from_gix)?;
    let time = author.time().map_err(GitError::from_gix)?;
    let message = commit.message_raw_sloppy();
    let summary = message
        .split(|&byte| byte == b'\n')
        .map(String::from_utf8_lossy)
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim_end().to_owned())
        .unwrap_or_default();
    Ok(CommitInfo {
        id: id.to_string(),
        short: id.to_hex_with_len(7).to_string(),
        author: author.name.to_string(),
        email: author.email.to_string(),
        time: time.seconds,
        offset_minutes: time.offset / 60,
        summary,
        message: String::from_utf8_lossy(message).into_owned(),
    })
}

/// A text's rows as git's line diff counts them: each ends at `\n`, and the
/// empty one after a final `\n` doesn't count. Compared without their `\r\n`
/// or `\n`.
struct Rows<'a>(Vec<&'a [u8]>);

impl<'a> TokenSource for Rows<'a> {
    type Token = &'a [u8];
    type Tokenizer = std::vec::IntoIter<&'a [u8]>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.clone().into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        self.0.len() as u32
    }
}

fn rows(data: &[u8]) -> Rows<'_> {
    if data.is_empty() {
        return Rows(Vec::new());
    }
    let data = data.strip_suffix(b"\n").unwrap_or(data);
    Rows(
        data.split(|&byte| byte == b'\n')
            .map(|row| row.strip_suffix(b"\r").unwrap_or(row))
            .collect(),
    )
}
