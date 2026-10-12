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
    /// Local changes to these files (absolute paths) differ on the target, so
    /// switching would lose them. Nothing was changed.
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

impl Repo {
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
        if !output.status.success() || name.starts_with('-') {
            return Err(SwitchError::InvalidName);
        }
        let args = ["switch", "-q", "-c", name].map(OsString::from);
        self.run_switch(&args)
    }

    /// Stashes local changes, untracked files included, then switches to
    /// `name`. The stash is left for the user to pop.
    pub fn stash_and_switch(&self, name: &str) -> Result<(), SwitchError> {
        self.resolve_target(name)?;
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
        self.switch(name, false)
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

/// The tab-indented file list under git's "would be overwritten" error.
fn overwritten_paths(stderr: &str) -> Vec<&str> {
    if !stderr.contains("would be overwritten") {
        return Vec::new();
    }
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix('\t'))
        .collect()
}
