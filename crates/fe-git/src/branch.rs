//! Switching and creating branches, through the `git` executable (see
//! [`crate::cli`]): gix has no checkout that carries local changes over and
//! refuses to overwrite them as `git switch` does.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::cli::{Access, run_raw};
use crate::{GitError, Repo};

/// Why a branch wasn't switched to or created.
#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    /// Local changes to these paths (absolute; files, or directories holding
    /// untracked files) differ on the target, so switching would lose them. Nothing was changed.
    #[error("local changes would be overwritten by switching branches ({} files)", paths.len())]
    WouldOverwrite { paths: Vec<PathBuf> },
    /// Not a valid branch name.
    #[error("invalid branch name")]
    InvalidName,
    /// No local or remote-tracking branch of that name.
    #[error("no such branch")]
    NotFound,
    /// Git failed for another reason; `output` is all it printed.
    #[error("git failed:\n{output}")]
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
        let shown = match target {
            BranchRef::Local(name) => name.clone(),
            BranchRef::Remote { remote, branch } => format!("{remote}/{branch}"),
        };
        self.resolve_ref(target)?;
        self.stash(&shown)?;
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

    /// Switches to the local branch `name`, or to a remote-tracking one
    /// (`origin/feature`): its local branch if one of that name exists, else a
    /// new one tracking it. Local changes that don't clash come along; with
    /// `force`, those that do are discarded. Blocks until git exits.
    pub fn switch(&self, name: &str, force: bool) -> Result<(), SwitchError> {
        let mut args: Vec<OsString> = ["switch", "-q"].map(OsString::from).into();
        if force {
            args.push("--discard-changes".into());
        }
        match self.resolve_target(name)? {
            Target::Local => args.push(name.into()),
            Target::Remote { local: Some(local) } => args.push(local.into()),
            Target::Remote { local: None } => {
                args.push("--track".into());
                args.push(name.into());
            }
        }
        self.run_switch(&args)
    }

    /// Makes branch `name` at HEAD and switches to it, carrying local changes.
    pub fn create_branch(&self, name: &str) -> Result<(), SwitchError> {
        let args = ["check-ref-format", "--branch", name].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        let expanded = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || name.starts_with('-') || expanded.trim() != name {
            return Err(SwitchError::InvalidName);
        }
        let args = ["switch", "-q", "-c", name].map(OsString::from);
        self.run_switch(&args)
    }

    /// Stashes local changes, untracked files included, then switches to
    /// `name`. The stash is left for the user to pop.
    pub fn stash_and_switch(&self, name: &str) -> Result<(), SwitchError> {
        self.resolve_target(name)?;
        self.stash(name)?;
        self.switch(name, false)
    }

    /// Stashes local changes, untracked files included, before switching
    /// to `name`.
    fn stash(&self, name: &str) -> Result<(), SwitchError> {
        let message = format!("fastedit: before switching to {name}");
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

    fn resolve_target(&self, name: &str) -> Result<Target, SwitchError> {
        if self.ref_exists(&format!("refs/heads/{name}"))? {
            return Ok(Target::Local);
        }
        if !self.ref_exists(&format!("refs/remotes/{name}"))? {
            return Err(SwitchError::NotFound);
        }
        let local = name.split_once('/').map(|(_, branch)| branch.to_owned());
        let local = match local {
            Some(branch) if self.ref_exists(&format!("refs/heads/{branch}"))? => Some(branch),
            _ => None,
        };
        Ok(Target::Remote { local })
    }

    fn ref_exists(&self, full_name: &str) -> Result<bool, SwitchError> {
        let args = ["show-ref", "--verify", "--quiet", full_name].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        Ok(output.status.success())
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

enum Target {
    Local,
    /// A remote-tracking branch, and the local branch of the same name if any.
    Remote {
        local: Option<String>,
    },
}

fn failed(output: &std::process::Output) -> SwitchError {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    SwitchError::Failed {
        output: text.trim().to_owned(),
    }
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
