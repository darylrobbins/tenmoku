//! Branches and HEAD: what the branch switcher lists and what amend pre-fills.

use std::collections::HashMap;
use std::ffi::OsString;

use gix::bstr::ByteSlice;
use gix::refs::Category;

use crate::cli::{self, Access};
use crate::{GitError, Repo};

/// A commit message's subject as git's `%s` gives it: the first paragraph
/// after any leading empty lines, each line's trailing whitespace dropped,
/// joined by single spaces.
pub(crate) fn subject(message: &[u8]) -> String {
    let lines = message
        .split(|&b| b == b'\n')
        .map(|line| line.trim_end())
        .skip_while(|line| line.is_empty())
        .take_while(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>();
    lines.join(" ")
}

/// Where HEAD points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// On a branch, by its short name.
    Branch(String),
    /// On a commit, by its first 7 hex digits.
    Detached(String),
    /// On a branch with no commits yet, by its name.
    Unborn(String),
}

/// Splits a remote-tracking short name into its remote and branch: the longest
/// configured remote name followed by `/` and a non-empty rest, else (a remote
/// since removed) the split at the first `/`.
pub(crate) fn split_remote<'a>(remotes: &[String], short: &'a str) -> Option<(&'a str, &'a str)> {
    remotes
        .iter()
        .filter_map(|remote| {
            let rest = short.strip_prefix(remote.as_str())?.strip_prefix('/')?;
            (!rest.is_empty()).then(|| (&short[..remote.len()], rest))
        })
        .max_by_key(|(remote, _)| remote.len())
        .or_else(|| short.split_once('/'))
}

/// A local or remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main` for a local branch, `origin/main` for a remote one.
    pub name: String,
    /// The configured remote's whole name for a remote-tracking branch, `None` for a local one.
    pub remote: Option<String>,
    pub is_current: bool,
    /// The short name of the remote-tracking branch a local branch tracks.
    pub upstream: Option<String>,
    /// The tip's committer time, seconds since the epoch.
    pub committed: i64,
    /// The tip's subject, as git's `%s` gives it.
    pub summary: String,
    /// Commits on a local branch that its upstream lacks, as of the last
    /// fetch. `None` for remote-tracking branches, branches without an
    /// upstream and branches whose upstream is gone.
    pub ahead: Option<u32>,
    /// Commits on the upstream that a local branch lacks, as of the last
    /// fetch; `None` in the same cases as `ahead`.
    pub behind: Option<u32>,
    /// A local branch's upstream is configured but its ref no longer exists
    /// (git's `[gone]`), as after a fetch with `--prune`.
    pub upstream_gone: bool,
}

/// What `%(upstream:track,nobracket)` says about one local branch.
#[derive(Default)]
struct Track {
    ahead: Option<u32>,
    behind: Option<u32>,
    gone: bool,
}

/// Parses `ahead N`, `behind M`, `ahead N, behind M`, `gone` or empty.
fn parse_track(text: &str) -> Track {
    let mut track = Track::default();
    if text.trim() == "gone" {
        track.gone = true;
        return track;
    }
    for part in text.split(',') {
        let mut words = part.split_whitespace();
        let (Some(word), Some(count)) = (words.next(), words.next()) else {
            continue;
        };
        let count = count.parse::<u32>().ok();
        match word {
            "ahead" => track.ahead = count,
            "behind" => track.behind = count,
            _ => {}
        }
    }
    track
}

impl Repo {
    /// Where HEAD points.
    pub fn current_branch(&self) -> Result<Head, GitError> {
        let repo = self.repo.to_thread_local();
        let head = repo.head().map_err(GitError::from_gix)?;
        let branch = head.referent_name().map(|name| name.shorten().to_string());
        Ok(match (branch, head.id()) {
            (Some(name), Some(_)) => Head::Branch(name),
            (Some(name), None) => Head::Unborn(name),
            (None, Some(id)) => Head::Detached(id.to_hex_with_len(7).to_string()),
            (None, None) => {
                return Err(GitError::Gix(
                    kiln_l10n::tr!("git-head-nowhere").to_string(),
                ));
            }
        })
    }

    /// Local branches then remote-tracking ones (never `<remote>/HEAD`), each
    /// group newest tip first by committer time, ties by name.
    pub fn branches(&self) -> Result<Vec<Branch>, GitError> {
        let repo = self.repo.to_thread_local();
        let current = repo
            .head()
            .map_err(GitError::from_gix)?
            .referent_name()
            .map(|name| name.as_bstr().to_owned());
        let remotes: Vec<String> = repo
            .remote_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        let mut tracks = self.upstream_tracks()?;
        let mut branches = Vec::new();
        let refs = repo.references().map_err(GitError::from_gix)?;
        let iter = refs.prefixed("refs/").map_err(GitError::from_gix)?;
        for reference in iter {
            let mut reference = reference.map_err(GitError::from_gix)?;
            let full_name = reference.name().to_owned();
            let Some((category, short)) = full_name.category_and_short_name() else {
                continue;
            };
            let remote = match category {
                Category::LocalBranch => None,
                Category::RemoteBranch => {
                    let short = short.to_string();
                    let Some((remote, branch)) = split_remote(&remotes, &short) else {
                        continue;
                    };
                    if branch == "HEAD" {
                        continue;
                    }
                    Some(remote.to_string())
                }
                _ => continue,
            };
            let Ok(commit) = reference.peel_to_commit() else {
                continue;
            };
            let committed = commit.time().map_err(GitError::from_gix)?.seconds;
            let summary = subject(commit.message_raw_sloppy());
            let upstream = if remote.is_none() {
                repo.branch_remote_tracking_ref_name(
                    full_name.as_ref(),
                    gix::remote::Direction::Fetch,
                )
                .and_then(Result::ok)
                .map(|name| name.shorten().to_string())
            } else {
                None
            };
            let (ahead, behind, upstream_gone) = match tracks.remove(&full_name.to_string()) {
                Some((has_upstream, track)) if remote.is_none() && has_upstream => {
                    if track.gone {
                        (None, None, true)
                    } else {
                        (
                            Some(track.ahead.unwrap_or(0)),
                            Some(track.behind.unwrap_or(0)),
                            false,
                        )
                    }
                }
                _ => (None, None, false),
            };
            branches.push(Branch {
                name: short.to_string(),
                remote,
                is_current: current.as_ref() == Some(&full_name.as_bstr().to_owned()),
                upstream,
                committed,
                summary,
                ahead,
                behind,
                upstream_gone,
            });
        }
        branches.sort_by(|a, b| {
            (a.remote.is_some(), std::cmp::Reverse(a.committed), &a.name).cmp(&(
                b.remote.is_some(),
                std::cmp::Reverse(b.committed),
                &b.name,
            ))
        });
        Ok(branches)
    }

    /// Each local branch's full ref name to whether it has an upstream and
    /// what git says about the distance, from one `for-each-ref` call.
    fn upstream_tracks(&self) -> Result<HashMap<String, (bool, Track)>, GitError> {
        let args: Vec<OsString> = [
            "for-each-ref",
            "--format=%(refname) %(upstream) %(upstream:track,nobracket)",
            "refs/heads",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let out = cli::run(&self.workdir, Access::Read, &args, None)?;
        let mut tracks = HashMap::new();
        for line in String::from_utf8_lossy(&out).lines() {
            let mut fields = line.splitn(3, ' ');
            let (Some(name), Some(upstream)) = (fields.next(), fields.next()) else {
                continue;
            };
            let track = parse_track(fields.next().unwrap_or(""));
            tracks.insert(name.to_owned(), (!upstream.is_empty(), track));
        }
        Ok(tracks)
    }

    /// `(ahead, behind)` of HEAD's branch against its upstream, as of the
    /// last fetch. `None` when HEAD is detached or unborn, or the branch has
    /// no upstream or its upstream is gone. Any other git failure, such as an
    /// upstream the fetch refspec doesn't map, is an error.
    pub fn upstream_counts(&self) -> Result<Option<(u32, u32)>, GitError> {
        if !matches!(self.current_branch()?, Head::Branch(_)) {
            return Ok(None);
        }
        let args: Vec<OsString> = ["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]
            .iter()
            .map(OsString::from)
            .collect();
        let out = match cli::run(&self.workdir, Access::Read, &args, None) {
            Ok(out) => out,
            Err(GitError::Command { stderr, .. })
                if stderr.contains("no upstream configured")
                    || stderr.contains("unknown revision")
                    || stderr.contains("no such branch") =>
            {
                return Ok(None);
            }
            Err(err) => return Err(err),
        };
        let text = String::from_utf8_lossy(&out);
        let mut numbers = text.split_whitespace().map(|n| n.parse::<u32>());
        match (numbers.next(), numbers.next()) {
            (Some(Ok(ahead)), Some(Ok(behind))) => Ok(Some((ahead, behind))),
            _ => Err(GitError::Gix(
                kiln_l10n::tr!("git-rev-list-unexpected", text = &*text).to_string(),
            )),
        }
    }

    /// Whether a merge or rebase is in progress, as VS Code's `headLabel`
    /// marks with `!`: the repository's own git dir (a linked worktree's,
    /// not the shared one) holds `MERGE_HEAD`, or `REBASE_HEAD` and a
    /// `rebase-merge` or `rebase-apply` directory. Runs no git process.
    pub fn merging(&self) -> bool {
        let repo = self.repo.to_thread_local();
        let dir = repo.git_dir();
        dir.join("MERGE_HEAD").exists()
            || (dir.join("REBASE_HEAD").exists()
                && (dir.join("rebase-merge").is_dir() || dir.join("rebase-apply").is_dir()))
    }

    /// HEAD commit's full message, `None` when HEAD is unborn.
    pub fn head_message(&self) -> Result<Option<String>, GitError> {
        let repo = self.repo.to_thread_local();
        let mut head = repo.head().map_err(GitError::from_gix)?;
        if head.is_unborn() {
            return Ok(None);
        }
        let commit = head.peel_to_commit().map_err(GitError::from_gix)?;
        Ok(Some(
            String::from_utf8_lossy(commit.message_raw().map_err(GitError::from_gix)?).into_owned(),
        ))
    }
}
