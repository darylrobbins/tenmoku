//! A branch's or every branch's commits with their parents and ref names, and
//! the files a commit changed, for the repository log. A log can be narrowed
//! to the commits a [`LogFilter`] matches.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use gix::ObjectId;

use crate::blame::commit_info;
use crate::cli::{self, Access};
use crate::{ChangeKind, CommitInfo, GitError, Repo};

/// Which commits a log lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogScope {
    /// HEAD's history (the current branch, or a detached HEAD).
    Head,
    /// Every local branch, remote-tracking branch and tag, and HEAD.
    All,
}

/// What narrows a log to the commits it matches: see [`LogFilter::matches`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    /// What's typed. Matched trimmed; empty, or only whitespace, matches
    /// every commit.
    pub text: String,
}

impl LogFilter {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// Whether it matches every commit: its text is empty but for
    /// whitespace.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Whether `commit` is one it keeps, ignoring case: each of its text's
    /// whitespace-separated words is in the commit's message, subject or
    /// body (as `git log -i --fixed-strings --all-match` with a `--grep`
    /// per word keeps them); or its whole text is in the author's name or
    /// email; or its text, of 4 or more hex digits, begins the commit's id.
    /// An empty filter keeps every commit.
    pub fn matches(&self, commit: &CommitInfo) -> bool {
        Matcher::new(self).is_none_or(|matcher| matcher.matches(commit))
    }
}

/// A non-empty [`LogFilter`], lower-cased once for the commits it's tried on.
struct Matcher {
    words: Vec<String>,
    text: String,
    /// The text, if it's a hex id prefix of 4 or more digits.
    id_prefix: Option<String>,
}

impl Matcher {
    /// None for an empty filter.
    fn new(filter: &LogFilter) -> Option<Self> {
        let text = filter.text.trim().to_lowercase();
        if text.is_empty() {
            return None;
        }
        let id_prefix = (text.len() >= 4 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| text.clone());
        Some(Self {
            words: text.split_whitespace().map(str::to_owned).collect(),
            text,
            id_prefix,
        })
    }

    fn matches(&self, commit: &CommitInfo) -> bool {
        if (self.id_prefix.as_ref()).is_some_and(|prefix| commit.id.starts_with(prefix.as_str())) {
            return true;
        }
        let contains = |field: &str| field.to_lowercase().contains(&self.text);
        if contains(&commit.author) || contains(&commit.email) {
            return true;
        }
        let message = commit.message.to_lowercase();
        self.words
            .iter()
            .all(|word| message.contains(word.as_str()))
    }
}

/// One commit of a log, as `git log --topo-order` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogCommit {
    pub commit: Arc<CommitInfo>,
    /// Full hex ids, first parent first; empty for a root commit.
    pub parents: Vec<String>,
    /// The refs pointing at this commit, in git's decoration order.
    pub refs: Vec<RefLabel>,
}

/// A ref pointing at a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    /// Short name: `main`, `origin/main`, `v1.0`, or `HEAD` for a detached HEAD.
    pub name: String,
    pub kind: RefKind,
    /// HEAD points at this branch (or, for [`RefKind::Head`], is detached here).
    pub is_head: bool,
}

/// What kind of ref a [`RefLabel`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// A detached HEAD.
    Head,
    /// A local branch.
    Branch,
    /// A remote-tracking branch.
    Remote,
    Tag,
}

/// A file a commit changed against its first parent (a root commit: against
/// the empty tree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    /// Absolute: the file's path in the commit (for a deletion, the path it had).
    pub path: PathBuf,
    /// Absolute: its path in the first parent, when the commit renamed it.
    pub from: Option<PathBuf>,
    /// Added, Modified (a rename included) or Deleted.
    pub kind: ChangeKind,
}

impl Repo {
    /// `limit` commits after the first `skip`, in `git log --topo-order`
    /// order, with parents and refs. Empty for an unborn HEAD or a `limit` of
    /// 0. Runs one `git log`; blocks until it exits.
    pub fn log(
        &self,
        scope: LogScope,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<LogCommit>, GitError> {
        let repo = self.repo.to_thread_local();
        if repo.head().map_err(GitError::from_gix)?.is_unborn() {
            return Ok(Vec::new());
        }
        let revisions: &[&str] = match scope {
            LogScope::Head => &["HEAD"],
            LogScope::All => &["--branches", "--remotes", "--tags", "HEAD"],
        };
        self.log_of(revisions, None, skip, limit)
    }

    /// As [`Repo::log`], of the commits `filter` matches alone: `limit` of
    /// them after the first `skip` that match, in the same order. An empty
    /// filter lists what [`Repo::log`] does. Runs one `git log`, listing
    /// the whole history for a non-empty filter, and reads each commit
    /// until `skip + limit` have matched; blocks until it's done.
    pub fn log_matching(
        &self,
        scope: LogScope,
        filter: &LogFilter,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<LogCommit>, GitError> {
        let repo = self.repo.to_thread_local();
        if repo.head().map_err(GitError::from_gix)?.is_unborn() {
            return Ok(Vec::new());
        }
        let revisions: &[&str] = match scope {
            LogScope::Head => &["HEAD"],
            LogScope::All => &["--branches", "--remotes", "--tags", "HEAD"],
        };
        self.log_of(revisions, Matcher::new(filter).as_ref(), skip, limit)
    }

    /// The ids `scope`'s log starts from, as they are now: HEAD's commit
    /// and, for [`LogScope::All`], each branch's, remote-tracking branch's
    /// and tag's (an annotated tag's own id). Empty for an unborn HEAD. Paging
    /// with [`Repo::log_from`] from them walks one fixed history, whatever
    /// the refs do meanwhile. Runs one `git rev-parse`; blocks until it exits.
    pub fn log_tips(&self, scope: LogScope) -> Result<Vec<String>, GitError> {
        let repo = self.repo.to_thread_local();
        if repo.head().map_err(GitError::from_gix)?.is_unborn() {
            return Ok(Vec::new());
        }
        let mut args: Vec<OsString> = vec!["rev-parse".into(), "HEAD".into()];
        if scope == LogScope::All {
            args.extend(["--branches", "--remotes", "--tags"].map(OsString::from));
        }
        let stdout = cli::run(&self.workdir, Access::Read, &args, None)?;
        let stdout = String::from_utf8_lossy(&stdout);
        let mut seen: HashSet<&str> = HashSet::new();
        let mut tips: Vec<String> = Vec::new();
        for tip in stdout.split_whitespace() {
            if seen.insert(tip) {
                tips.push(tip.to_owned());
            }
        }
        Ok(tips)
    }

    /// As [`Repo::log`], from `tips` ([`Repo::log_tips`]) rather than the
    /// refs: `limit` commits after the first `skip` of the history they
    /// reach, which stays the same however the refs move, HEAD becoming
    /// unborn included. Empty for no tips or a `limit` of 0.
    pub fn log_from(
        &self,
        tips: &[String],
        skip: usize,
        limit: usize,
    ) -> Result<Vec<LogCommit>, GitError> {
        if tips.is_empty() {
            return Ok(Vec::new());
        }
        let tips: Vec<&str> = tips.iter().map(String::as_str).collect();
        self.log_of(&tips, None, skip, limit)
    }

    /// As [`Repo::log_from`], of the commits `filter` matches alone, as
    /// [`Repo::log_matching`] pages them.
    pub fn log_from_matching(
        &self,
        tips: &[String],
        filter: &LogFilter,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<LogCommit>, GitError> {
        if tips.is_empty() {
            return Ok(Vec::new());
        }
        let tips: Vec<&str> = tips.iter().map(String::as_str).collect();
        self.log_of(&tips, Matcher::new(filter).as_ref(), skip, limit)
    }

    /// `git log --topo-order` of `revisions`, as [`Repo::log`] reads it,
    /// whatever HEAD is; with a `matcher`, of the commits it matches alone,
    /// `skip` and `limit` counting those.
    fn log_of(
        &self,
        revisions: &[&str],
        matcher: Option<&Matcher>,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<LogCommit>, GitError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let repo = self.repo.to_thread_local();
        // Explicit flags, so neither the user's nor the repo's config changes
        // the output. The decoration patterns override log.excludeDecoration.
        let mut args: Vec<OsString> = [
            "-c",
            "log.showSignature=false",
            "log",
            "--topo-order",
            "-z",
            "--no-color",
            "--no-show-signature",
            "--no-abbrev",
            "--no-abbrev-commit",
            "--decorate=full",
            "--decorate-refs=HEAD",
            "--decorate-refs=refs/heads/",
            "--decorate-refs=refs/remotes/",
            "--decorate-refs=refs/tags/",
            "--format=%H%x1f%P%x1f%D",
            "--stdin",
        ]
        .map(OsString::from)
        .into();
        // A filter is matched here, so git lists the whole history.
        if matcher.is_none() {
            args.push(format!("--skip={skip}").into());
            args.push(format!("--max-count={limit}").into());
        }
        // Revisions go on stdin, past the argument-list limit; pseudo-options
        // (`--branches`, ...) stay on the command line, which `--stdin` takes
        // only from git 2.42.
        let mut stdin = String::new();
        for revision in revisions {
            if revision.starts_with("--") {
                args.push(revision.into());
            } else {
                stdin.push_str(revision);
                stdin.push('\n');
            }
        }
        args.push("--".into());
        let stdout = match cli::run(&self.workdir, Access::Read, &args, Some(stdin.as_bytes())) {
            Ok(stdout) => stdout,
            Err(GitError::Command { stderr, .. })
                if stderr.contains("does not have any commits") =>
            {
                return Ok(Vec::new());
            }
            Err(err) => return Err(err),
        };

        let mut commits = Vec::new();
        let mut matched = 0;
        for record in stdout.split(|&byte| byte == 0) {
            if commits.len() == limit {
                break;
            }
            let record = String::from_utf8_lossy(record);
            // Records after the first start with the line break ending the one before.
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                continue;
            }
            let mut fields = record.split('\x1f');
            let (Some(id), Some(parents), Some(refs)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let object_id = ObjectId::from_hex(id.as_bytes()).map_err(GitError::from_gix)?;
            let commit = commit_info(&repo, object_id)?;
            if let Some(matcher) = matcher {
                if !matcher.matches(&commit) {
                    continue;
                }
                matched += 1;
                if matched <= skip {
                    continue;
                }
            }
            commits.push(LogCommit {
                commit: Arc::new(commit),
                parents: parents.split_whitespace().map(str::to_owned).collect(),
                refs: ref_labels(refs.trim_end()),
            });
        }
        Ok(commits)
    }

    /// The files `commit` (a full or short id) changed against its first
    /// parent, renames detected, by path. A merge is compared with its first
    /// parent only. Runs one `git diff-tree`; blocks until it exits.
    pub fn commit_files(&self, commit: &str) -> Result<Vec<CommitFile>, GitError> {
        let repo = self.repo.to_thread_local();
        let prefix = gix::hash::Prefix::from_hex(commit).map_err(GitError::from_gix)?;
        let id = match repo
            .objects
            .lookup_prefix(prefix, None)
            .map_err(GitError::from_gix)?
        {
            Some(Ok(id)) => id,
            Some(Err(())) => {
                return Err(GitError::Gix(
                    kiln_l10n::tr!("git-commit-ambiguous", commit = commit.to_string()).to_string(),
                ));
            }
            None => {
                return Err(GitError::Gix(
                    kiln_l10n::tr!("git-commit-missing", commit = commit.to_string()).to_string(),
                ));
            }
        };
        let commit = repo.find_commit(id).map_err(GitError::from_gix)?;
        let mut args: Vec<OsString> = [
            "diff-tree",
            "-r",
            "-z",
            "-M",
            "--name-status",
            "--no-relative",
            "--no-commit-id",
        ]
        .map(OsString::from)
        .into();
        match commit.parent_ids().next() {
            Some(parent) => args.push(parent.to_string().into()),
            None => args.push("--root".into()),
        }
        args.push(commit.id.to_string().into());
        let stdout = cli::run(&self.workdir, Access::Read, &args, None)?;

        let mut files = Vec::new();
        let mut tokens = stdout.split(|&byte| byte == 0);
        while let Some(status) = tokens.next() {
            let Some(&letter) = status.first() else {
                continue;
            };
            let Some(first) = tokens.next() else { break };
            let (from, path) = if matches!(letter, b'R' | b'C') {
                let Some(second) = tokens.next() else { break };
                (Some(path_of(first)), path_of(second))
            } else {
                (None, path_of(first))
            };
            let kind = match letter {
                b'A' | b'C' => ChangeKind::Added,
                b'D' => ChangeKind::Deleted,
                _ => ChangeKind::Modified,
            };
            files.push(CommitFile {
                path: self.workdir.join(path),
                from: from
                    .filter(|_| letter == b'R')
                    .map(|from| self.workdir.join(from)),
                kind,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }
}

pub(crate) fn path_of(bytes: &[u8]) -> PathBuf {
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

/// Parses `%D` with `--decorate=full`: `HEAD -> refs/heads/main,
/// refs/remotes/origin/main, tag: refs/tags/v1`, or a bare `HEAD` when HEAD is
/// detached here. `origin/HEAD` and refs outside heads, remotes and tags are
/// left out.
fn ref_labels(decoration: &str) -> Vec<RefLabel> {
    let mut labels = Vec::new();
    for item in decoration.split(", ").filter(|item| !item.is_empty()) {
        if item == "HEAD" {
            labels.push(RefLabel {
                name: "HEAD".to_owned(),
                kind: RefKind::Head,
                is_head: true,
            });
            continue;
        }
        let (full, is_head) = match item.strip_prefix("HEAD -> ") {
            Some(full) => (full, true),
            None => (item, false),
        };
        let (name, kind) = if let Some(name) = full.strip_prefix("tag: refs/tags/") {
            (name, RefKind::Tag)
        } else if let Some(name) = full.strip_prefix("refs/heads/") {
            (name, RefKind::Branch)
        } else if let Some(name) = full.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            (name, RefKind::Remote)
        } else {
            continue;
        };
        labels.push(RefLabel {
            name: name.to_owned(),
            kind,
            is_head,
        });
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decorations() {
        let label = |name: &str, kind, is_head| RefLabel {
            name: name.to_owned(),
            kind,
            is_head,
        };
        assert_eq!(
            ref_labels(
                "HEAD -> refs/heads/main, refs/remotes/origin/main, \
                 refs/remotes/origin/HEAD, tag: refs/tags/v1"
            ),
            [
                label("main", RefKind::Branch, true),
                label("origin/main", RefKind::Remote, false),
                label("v1", RefKind::Tag, false),
            ]
        );
        assert_eq!(
            ref_labels("HEAD, refs/heads/topic/x"),
            [
                label("HEAD", RefKind::Head, true),
                label("topic/x", RefKind::Branch, false),
            ]
        );
        assert!(ref_labels("").is_empty());
    }
}
