//! Switching, creating, renaming, deleting and merging branches, through the
//! `git` executable (see [`crate::cli`]): gix has no checkout that carries
//! local changes over and refuses to overwrite them as `git switch` does.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Output;

use crate::cli::{Access, run_raw};
use crate::{GitError, PullMode, Repo};

/// Why a branch wasn't switched to or created.
#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    /// Local changes to these paths (absolute; files, or directories holding
    /// untracked files) differ on the target, so switching would lose them. Nothing was changed.
    #[error("{}", kiln_l10n::tr!("git-switch-would-overwrite", count = paths.len()))]
    WouldOverwrite { paths: Vec<PathBuf> },
    /// Not a valid branch name.
    #[error("{}", kiln_l10n::tr!("git-switch-invalid-name"))]
    InvalidName,
    /// No local or remote-tracking branch of that name.
    #[error("{}", kiln_l10n::tr!("git-switch-not-found"))]
    NotFound,
    /// A local branch of the new branch's name exists already. Nothing was
    /// changed.
    #[error("{}", kiln_l10n::tr!("git-branch-exists"))]
    Exists,
    /// Git failed for another reason; `output` is all it printed.
    #[error("{}", kiln_l10n::tr!("git-switch-failed", output = output.as_str()))]
    Failed { output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// Why a branch wasn't renamed or deleted. Nothing was changed.
#[derive(Debug, thiserror::Error)]
pub enum BranchError {
    /// Not a valid branch name.
    #[error("{}", kiln_l10n::tr!("git-switch-invalid-name"))]
    InvalidName,
    /// No local branch of that name.
    #[error("{}", kiln_l10n::tr!("git-switch-not-found"))]
    NotFound,
    /// A local branch of the new name exists already.
    #[error("{}", kiln_l10n::tr!("git-branch-exists"))]
    Exists,
    /// The branch is HEAD's, which can't be deleted.
    #[error("{}", kiln_l10n::tr!("git-branch-current"))]
    Current,
    /// The branch has commits that its upstream, or HEAD when it has none,
    /// doesn't, as `git branch -d` judges; deleting it with `force` drops
    /// them.
    #[error("{}", kiln_l10n::tr!("git-branch-not-merged"))]
    NotMerged,
    /// Git failed for another reason (a branch checked out in another
    /// worktree, say); `output` is all it printed.
    #[error("{}", kiln_l10n::tr!("git-branch-failed", output = output.as_str()))]
    Failed { output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// Why a branch wasn't merged into HEAD's, or stopped part way.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// No branch of that name.
    #[error("{}", kiln_l10n::tr!("git-switch-not-found"))]
    NotFound,
    /// HEAD isn't on a branch. Nothing was changed.
    #[error("{}", kiln_l10n::tr!("git-remote-detached"))]
    Detached,
    /// Local changes to these paths (absolute) would be overwritten by the
    /// merge. Nothing was changed.
    #[error("{}", kiln_l10n::tr!("git-merge-would-overwrite", count = paths.len()))]
    WouldOverwrite { paths: Vec<PathBuf> },
    /// The merge stopped on conflicts in these files (absolute paths), and is
    /// left in progress for the user to resolve or abort.
    #[error("{}", kiln_l10n::tr!("git-remote-conflicts", count = paths.len()))]
    Conflicts { paths: Vec<PathBuf> },
    /// Git failed for another reason (a merge already in progress, say), and
    /// `output` is all it printed; or it wasn't run because it would have
    /// signed, and `output` says so.
    #[error("{}", kiln_l10n::tr!("git-merge-failed", output = output.as_str()))]
    Failed { output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// A branch to switch to, by the kind of ref it is, so a short name both
/// kinds have (a local `origin/feat` and the remote one) can't be taken for
/// the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchRef {
    /// A local branch, by its short name.
    Local(String),
    /// A remote-tracking branch: its configured remote's whole name and the
    /// branch on it, `team/origin` and `feat` for `team/origin/feat`.
    Remote { remote: String, branch: String },
}

impl BranchRef {
    /// Its full ref name, `refs/heads/feat` or `refs/remotes/origin/feat`.
    fn full_name(&self) -> String {
        match self {
            BranchRef::Local(name) => format!("refs/heads/{name}"),
            BranchRef::Remote { remote, branch } => format!("refs/remotes/{remote}/{branch}"),
        }
    }

    /// Its short name, `feat` or `origin/feat`, as git shows it.
    fn short_name(&self) -> String {
        match self {
            BranchRef::Local(name) => name.clone(),
            BranchRef::Remote { remote, branch } => format!("{remote}/{branch}"),
        }
    }
}

impl Repo {
    /// Switches to `target`: a local branch, or for a remote-tracking one
    /// the local branch of its name if one exists, else a new one tracking
    /// it. As [`Repo::switch`] otherwise.
    pub fn switch_to(&self, target: &BranchRef, force: bool) -> Result<(), SwitchError> {
        let mut args: Vec<OsString> = ["switch", "-q"].map(OsString::from).into();
        if force {
            args.push("--discard-changes".into());
        }
        match self.resolve_ref(target)? {
            Some(local) => args.push(local.into()),
            None => {
                let BranchRef::Remote { remote, branch } = target else {
                    unreachable!("a local target resolves to itself");
                };
                let tracked = format!("refs/remotes/{remote}/{branch}");
                args.extend(["-c".into(), branch.into(), "--track".into(), tracked.into()]);
            }
        }
        self.run_switch(&args)
    }

    /// Stashes local changes, untracked files included, then switches to
    /// `target` as [`Repo::switch_to`] does. The stash is left for the user
    /// to pop.
    pub fn stash_and_switch_to(&self, target: &BranchRef) -> Result<(), SwitchError> {
        self.resolve_ref(target)?;
        self.stash(&target.short_name())?;
        self.switch_to(target, false)
    }

    /// The local branch `target` switches to: none for a remote-tracking
    /// one with no local branch of its name, which a switch creates.
    fn resolve_ref(&self, target: &BranchRef) -> Result<Option<String>, SwitchError> {
        let (full, local) = match target {
            BranchRef::Local(name) => (format!("refs/heads/{name}"), name),
            BranchRef::Remote { remote, branch } => {
                (format!("refs/remotes/{remote}/{branch}"), branch)
            }
        };
        if !self.ref_exists(&full)? {
            return Err(SwitchError::NotFound);
        }
        let is_local = matches!(target, BranchRef::Local(_))
            || self.ref_exists(&format!("refs/heads/{local}"))?;
        Ok(is_local.then(|| local.clone()))
    }

    /// The branch a typed short name means: the local branch of that name
    /// first, else the remote-tracking one, split at its longest configured
    /// remote name (`team/origin/feat` is `feat` on `team/origin`).
    pub fn resolve_branch(&self, name: &str) -> Result<BranchRef, SwitchError> {
        if self.ref_exists(&format!("refs/heads/{name}"))? {
            return Ok(BranchRef::Local(name.to_owned()));
        }
        if !self.ref_exists(&format!("refs/remotes/{name}"))? {
            return Err(SwitchError::NotFound);
        }
        let remotes: Vec<String> = self
            .repo
            .to_thread_local()
            .remote_names()
            .iter()
            .map(|remote| remote.to_string())
            .collect();
        let (remote, branch) =
            crate::refs::split_remote(&remotes, name).ok_or(SwitchError::NotFound)?;
        Ok(BranchRef::Remote {
            remote: remote.to_owned(),
            branch: branch.to_owned(),
        })
    }

    /// Switches to the local branch `name`, or to a remote-tracking one
    /// (`origin/feature`): its local branch if one of that name exists, else a
    /// new one tracking it. Local changes that don't clash come along; with
    /// `force`, those that do are discarded. Blocks until git exits.
    pub fn switch(&self, name: &str, force: bool) -> Result<(), SwitchError> {
        let target = self.resolve_branch(name)?;
        self.switch_to(&target, force)
    }

    /// Makes branch `name` at HEAD and switches to it, carrying local changes.
    pub fn create_branch(&self, name: &str) -> Result<(), SwitchError> {
        if !self.is_branch_name(name)? {
            return Err(SwitchError::InvalidName);
        }
        let args = ["switch", "-q", "-c", name].map(OsString::from);
        self.run_switch(&args)
    }

    /// Makes branch `name` at `start`, tracking it when it's a
    /// remote-tracking branch and not when it's a local one, whatever
    /// `branch.autoSetupMerge` says (`git branch --track` / `--no-track`).
    /// With `switch`, also switches to it, carrying local changes as
    /// [`Repo::switch`] does (`git switch -c`); a clash leaves no branch made.
    pub fn create_branch_at(
        &self,
        name: &str,
        start: &BranchRef,
        switch: bool,
    ) -> Result<(), SwitchError> {
        if !self.is_branch_name(name)? {
            return Err(SwitchError::InvalidName);
        }
        let full = start.full_name();
        if !self.ref_exists(&full)? {
            return Err(SwitchError::NotFound);
        }
        if self.local_exists(name)? {
            return Err(SwitchError::Exists);
        }
        let track = match start {
            BranchRef::Local(_) => "--no-track",
            BranchRef::Remote { .. } => "--track",
        };
        if switch {
            let args = ["switch", "-q", "-c", name, track, &full].map(OsString::from);
            return self.run_switch(&args);
        }
        let args = ["branch", "-q", track, name, &full].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if !output.status.success() {
            return Err(failed(&output));
        }
        Ok(())
    }

    /// Renames local branch `old` to `new`, which must be a valid name no
    /// branch has (`git branch -m`). Its upstream and other config, its
    /// reflog, and any HEAD on it move along.
    pub fn rename_branch(&self, old: &str, new: &str) -> Result<(), BranchError> {
        if !self.local_exists(old)? {
            return Err(BranchError::NotFound);
        }
        if !self.is_branch_name(new)? {
            return Err(BranchError::InvalidName);
        }
        if old == new {
            return Ok(());
        }
        if self.local_exists(new)? {
            return Err(BranchError::Exists);
        }
        let args = ["branch", "-m", "--", old, new].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if !output.status.success() {
            return Err(BranchError::Failed {
                output: output_text(&output),
            });
        }
        Ok(())
    }

    /// Deletes local branch `name` and its config (`git branch -d`), or with
    /// `force` even when it isn't merged (`-D`). HEAD's branch is refused.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<(), BranchError> {
        if !self.local_exists(name)? {
            return Err(BranchError::NotFound);
        }
        if self.branch()?.as_deref() == Some(name) {
            return Err(BranchError::Current);
        }
        let flag = if force { "-D" } else { "-d" };
        let args = ["branch", flag, "--", name].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if output.status.success() {
            return Ok(());
        }
        if !force && String::from_utf8_lossy(&output.stderr).contains("is not fully merged") {
            return Err(BranchError::NotMerged);
        }
        Err(BranchError::Failed {
            output: output_text(&output),
        })
    }

    /// Merges `source` into HEAD's branch, fast-forwarding when possible,
    /// with git's own message (`git merge --no-edit --ff`); a remote-tracking
    /// branch is merged as last fetched. Local changes are never stashed:
    /// those the merge would overwrite stop it. Conflicts leave the merge in
    /// progress, as [`Repo::pull`] with [`PullMode::Merge`] does. Nothing is
    /// signed: when the merge would sign (`commit.gpgSign`, or
    /// `branch.<name>.mergeOptions`), one that needs a commit is
    /// [`MergeError::Failed`] with nothing changed, and a fast-forward goes
    /// ahead. Hooks run, and signatures are verified, as configured.
    pub fn merge_into_current(&self, source: &BranchRef) -> Result<(), MergeError> {
        if !self.ref_exists(&source.full_name())? {
            return Err(MergeError::NotFound);
        }
        let branch = self.branch()?.ok_or(MergeError::Detached)?;
        let signs = self.signs_commits(&branch, PullMode::Merge)?;
        // Conflicts already in the index are an earlier merge's: git refuses
        // to start this one.
        let unmerged_before = !self.unmerged_paths()?.is_empty();
        let ff = if signs { "--ff-only" } else { "--ff" };
        let mut args: Vec<OsString> = [
            "merge",
            "-q",
            "--no-edit",
            "--no-autostash",
            "--no-gpg-sign",
            ff,
            "--",
        ]
        .map(OsString::from)
        .into();
        args.push(self.merge_name(source)?.into());
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if !unmerged_before {
            let paths = self.unmerged_paths()?;
            if !paths.is_empty() {
                return Err(MergeError::Conflicts { paths });
            }
        }
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let paths = overwritten_paths(&stderr);
        if !paths.is_empty() {
            return Err(MergeError::WouldOverwrite {
                paths: paths.iter().map(|path| self.workdir.join(path)).collect(),
            });
        }
        if signs && stderr.contains("Not possible to fast-forward") {
            return Err(MergeError::Failed {
                output: kiln_l10n::tr!("git-signing-commit").to_string(),
            });
        }
        Err(MergeError::Failed {
            output: output_text(&output),
        })
    }

    /// What to name `source` to git merge: its short name when that means
    /// it, so the message reads `Merge branch 'feat'`, else its full ref name
    /// (a tag or a local branch of the same short name would win).
    fn merge_name(&self, source: &BranchRef) -> Result<String, GitError> {
        let short = source.short_name();
        let full = source.full_name();
        if short.starts_with('-') {
            return Ok(full);
        }
        let args = [
            "rev-parse",
            "--verify",
            "-q",
            "--symbolic-full-name",
            &short,
        ]
        .map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        let means =
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == full;
        Ok(if means { short } else { full })
    }

    /// Whether `name` is a branch name git would make as written: valid, and
    /// not shorthand it expands (`@{-1}`) or an option.
    fn is_branch_name(&self, name: &str) -> Result<bool, GitError> {
        let args = ["check-ref-format", "--branch", name].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        let expanded = String::from_utf8_lossy(&output.stdout);
        Ok(output.status.success() && !name.starts_with('-') && expanded.trim() == name)
    }

    /// Stashes local changes, untracked files included, then switches to
    /// `name`. The stash is left for the user to pop.
    pub fn stash_and_switch(&self, name: &str) -> Result<(), SwitchError> {
        let target = self.resolve_branch(name)?;
        self.stash_and_switch_to(&target)
    }

    /// Stashes local changes, untracked files included, before switching
    /// to `name`.
    fn stash(&self, name: &str) -> Result<(), SwitchError> {
        let message = format!("tenmoku: before switching to {name}");
        let args: Vec<OsString> = ["stash", "push", "--include-untracked", "-m"]
            .map(OsString::from)
            .into_iter()
            .chain([message.into()])
            .collect();
        let output = run_raw(&self.workdir, Access::Write, &args, None, &[])?;
        if !output.status.success() {
            return Err(failed(&output));
        }
        Ok(())
    }

    fn ref_exists(&self, full_name: &str) -> Result<bool, GitError> {
        let args = ["show-ref", "--verify", "--quiet", full_name].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        Ok(output.status.success())
    }

    /// Whether local branch `name` exists.
    fn local_exists(&self, name: &str) -> Result<bool, GitError> {
        self.ref_exists(&format!("refs/heads/{name}"))
    }

    fn run_switch(&self, args: &[OsString]) -> Result<(), SwitchError> {
        let output = run_raw(&self.workdir, Access::Write, args, None, &[])?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let paths = overwritten_paths(&stderr);
        if !paths.is_empty() {
            return Err(SwitchError::WouldOverwrite {
                paths: paths.iter().map(|path| self.workdir.join(path)).collect(),
            });
        }
        Err(failed(&output))
    }
}

fn failed(output: &Output) -> SwitchError {
    SwitchError::Failed {
        output: output_text(output),
    }
}

/// All that git printed, stdout then stderr.
fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.trim().to_owned()
}

/// The tab-indented path list under git's "would be overwritten" and
/// "would lose untracked files in them" errors.
fn overwritten_paths(stderr: &str) -> Vec<&str> {
    if !stderr.contains("would be overwritten")
        && !stderr.contains("would lose untracked files in them")
    {
        return Vec::new();
    }
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix('\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("run git");
        assert!(output.status.success(), "git {args:?} failed");
    }

    #[test]
    fn resolve_branch_by_the_full_remote_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(
            root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "m",
            ],
        );
        git(root, &["remote", "add", "team/origin", "/nonexistent"]);
        git(
            root,
            &["update-ref", "refs/remotes/team/origin/feat", "HEAD"],
        );
        git(root, &["branch", "-q", "origin/feat"]);
        let repo = Repo::discover(root).unwrap().expect("a repository");
        assert_eq!(
            repo.resolve_branch("team/origin/feat").unwrap(),
            BranchRef::Remote {
                remote: "team/origin".into(),
                branch: "feat".into()
            }
        );
        // A local name wins over a remote one of the same short name.
        git(root, &["remote", "add", "origin", "/nonexistent"]);
        git(root, &["update-ref", "refs/remotes/origin/feat", "HEAD"]);
        assert_eq!(
            repo.resolve_branch("origin/feat").unwrap(),
            BranchRef::Local("origin/feat".into())
        );
        assert!(matches!(
            repo.resolve_branch("nope"),
            Err(SwitchError::NotFound)
        ));
    }
}
