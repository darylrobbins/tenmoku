//! Stashing local changes and bringing them back, through the `git`
//! executable (see [`crate::cli`]), as VS Code's Git: Stash, Pop Stash and
//! Apply Stash commands run it.

use std::ffi::OsString;
use std::process::Output;

use crate::cli::{Access, run, run_raw};
use crate::{GitError, Repo};

/// What `git stash push` prints, exiting 0, when there's nothing to stash.
const NOTHING_TO_SAVE: &str = "No local changes to save";

/// Why a stash wasn't made, popped or applied.
#[derive(Debug, thiserror::Error)]
pub enum StashError {
    /// The working tree and index match HEAD (and, unless untracked files
    /// were included, there's nothing else): no entry was made.
    #[error("No local changes to save")]
    NothingToStash,
    /// The entry was applied, but clashed with the working tree: the files
    /// hold conflict markers, and the entry is kept, popped or not.
    /// `conflicts` are git's `CONFLICT (…)` lines, naming each file.
    #[error("conflicts applying the stash; the stash entry is kept:\n{}", conflicts.join("\n"))]
    Conflict { conflicts: Vec<String> },
    /// Git failed for another reason, changing nothing; `output` is all it
    /// printed.
    #[error("git stash failed:\n{output}")]
    Failed { output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// An entry on the stash, `stash@{index}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    /// Its place on the stash, the latest 0.
    pub index: usize,
    /// Its commit's full id, which stays the same as entries above it come
    /// and go.
    pub id: String,
    /// The message it was stashed with, or git's own, `<short id>
    /// <subject>` of the commit it was made on, when given none.
    pub message: String,
    /// The branch it was made on, `(no branch)` when HEAD was detached;
    /// `None` for an entry stored with a message of its own.
    pub branch: Option<String>,
}

impl Repo {
    /// Stashes the local changes, staged and not, with `message` if given,
    /// and untracked files too if `include_untracked`; the working tree
    /// then matches HEAD. Blocks until git exits.
    pub fn stash_push(
        &self,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<(), StashError> {
        let mut args: Vec<OsString> = ["stash", "push"].map(OsString::from).into();
        if include_untracked {
            args.push("--include-untracked".into());
        }
        if let Some(message) = message {
            args.extend(["-m".into(), message.into()]);
        }
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if !output.status.success() {
            return Err(failed(&output));
        }
        if String::from_utf8_lossy(&output.stdout).contains(NOTHING_TO_SAVE) {
            return Err(StashError::NothingToStash);
        }
        Ok(())
    }

    /// The entries on the stash, the latest first.
    pub fn stash_list(&self) -> Result<Vec<StashEntry>, GitError> {
        let args = ["stash", "list", "-z", "--format=%H %gs"].map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        let stdout = String::from_utf8_lossy(&stdout);
        let entries = (stdout.split('\0'))
            .map(|record| record.trim_start_matches('\n'))
            .filter_map(|record| record.split_once(' '))
            .enumerate()
            .map(|(index, (id, subject))| entry(index, id, subject))
            .collect();
        Ok(entries)
    }

    /// Applies entry `index` (`stash@{index}`) and drops it from the stash.
    /// One that conflicts is applied with markers and kept. Blocks until
    /// git exits.
    pub fn stash_pop(&self, index: usize) -> Result<(), StashError> {
        self.run_stash("pop", index)
    }

    /// Applies entry `index` (`stash@{index}`), keeping it on the stash.
    /// Blocks until git exits.
    pub fn stash_apply(&self, index: usize) -> Result<(), StashError> {
        self.run_stash("apply", index)
    }

    fn run_stash(&self, verb: &str, index: usize) -> Result<(), StashError> {
        let args = ["stash".into(), verb.into(), format!("stash@{{{index}}}")].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if output.status.success() {
            return Ok(());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let conflicts: Vec<String> = (stdout.lines())
            .filter(|line| line.starts_with("CONFLICT ("))
            .map(str::to_owned)
            .collect();
        if !conflicts.is_empty() {
            return Err(StashError::Conflict { conflicts });
        }
        Err(failed(&output))
    }
}

/// Entry `index`, commit `id`, from its reflog subject: `On <branch>:
/// <message>`, or `WIP on <branch>: <short id> <subject>` for one given no
/// message.
fn entry(index: usize, id: &str, subject: &str) -> StashEntry {
    // A branch name can't hold a colon.
    let parsed = (subject
        .strip_prefix("On ")
        .or_else(|| subject.strip_prefix("WIP on ")))
    .and_then(|rest| rest.split_once(": "));
    match parsed {
        Some((branch, message)) => StashEntry {
            index,
            id: id.to_owned(),
            message: message.to_owned(),
            branch: Some(branch.to_owned()),
        },
        None => StashEntry {
            index,
            id: id.to_owned(),
            message: subject.to_owned(),
            branch: None,
        },
    }
}

fn failed(output: &Output) -> StashError {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    StashError::Failed {
        output: text.trim().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects() {
        let parsed = |subject| {
            let entry = entry(2, "abc", subject);
            (entry.message, entry.branch)
        };
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(parsed("On main: wip"), ("wip".into(), some("main")));
        assert_eq!(
            parsed("WIP on feat/x: 1a2b3c4 init: the start"),
            ("1a2b3c4 init: the start".into(), some("feat/x"))
        );
        assert_eq!(
            parsed("On (no branch): a: b"),
            ("a: b".into(), some("(no branch)"))
        );
        assert_eq!(parsed("stored by hand"), ("stored by hand".into(), None));
        let entry = entry(2, "abc", "On main: wip");
        assert_eq!((entry.index, entry.id.as_str()), (2, "abc"));
    }
}
