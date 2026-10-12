//! Branches and HEAD: what the branch switcher lists and what amend pre-fills.

use gix::bstr::ByteSlice;
use gix::refs::Category;

use crate::{GitError, Repo};

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

/// A local or remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main` for a local branch, `origin/main` for a remote one.
    pub name: String,
    /// The remote's name for a remote-tracking branch, `None` for a local one.
    pub remote: Option<String>,
    pub is_current: bool,
    /// The short name of the remote-tracking branch a local branch tracks.
    pub upstream: Option<String>,
    /// The tip's committer time, seconds since the epoch.
    pub committed: i64,
    /// The first line of the tip's message.
    pub summary: String,
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
            (None, None) => return Err(GitError::Gix("HEAD points nowhere".into())),
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
                    let Some((remote, branch)) = short
                        .to_string()
                        .split_once('/')
                        .map(|(remote, branch)| (remote.to_string(), branch.to_string()))
                    else {
                        continue;
                    };
                    if branch == "HEAD" {
                        continue;
                    }
                    Some(remote)
                }
                _ => continue,
            };
            let Ok(commit) = reference.peel_to_commit() else {
                continue;
            };
            let committed = commit.time().map_err(GitError::from_gix)?.seconds;
            let summary = commit
                .message_raw_sloppy()
                .lines()
                .next()
                .map(|line| String::from_utf8_lossy(line).trim_end().to_owned())
                .unwrap_or_default();
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
            branches.push(Branch {
                name: short.to_string(),
                remote,
                is_current: current.as_ref() == Some(&full_name.as_bstr().to_owned()),
                upstream,
                committed,
                summary,
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
