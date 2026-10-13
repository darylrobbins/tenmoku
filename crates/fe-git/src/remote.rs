//! Fetching, pulling and pushing, through the `git` executable (see
//! [`crate::cli`]), so remotes, credential helpers and `ssh` behave as in a
//! terminal. Every call blocks until git exits, which can take as long as the
//! network does; callers run it off the UI thread.
//!
//! Nothing here waits on a prompt: git gets no stdin, terminal and askpass
//! prompts are off, and `ssh` runs in batch mode unless the user configured
//! their own ssh command. A remote that wants credentials it can't get
//! fails with [`RemoteError::Auth`]. A signer may ask for a passphrase, so
//! nothing here signs: a pull that would make a signed commit and a push with
//! `push.gpgSign` on are [`RemoteError::Failed`] before git runs them, and
//! every signer git could still reach, in a submodule say, is `false`. Pulls
//! don't recurse into submodules, so they never stash or rebase in one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use crate::cli::{Access, run, run_raw};
use crate::{GitError, Repo};

/// How [`Repo::pull`] brings in the upstream's commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    /// Merge them, fast-forwarding when possible (`git pull --no-rebase --ff`).
    Merge,
    /// Replay local commits on top of them (`git pull --rebase`).
    Rebase,
    /// Fast-forward only (`git pull --ff-only`).
    FastForwardOnly,
}

/// Why a fetch, pull or push didn't happen, or stopped part way.
#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    /// HEAD's branch has no upstream remote and there is no `origin`.
    #[error("no remote to fetch from")]
    NoRemote,
    /// HEAD's branch has no upstream to pull from or push to.
    #[error("the current branch has no upstream branch")]
    NoUpstream,
    /// HEAD isn't on a branch.
    #[error("HEAD is detached")]
    Detached,
    /// The pull stopped on conflicts in these files (absolute paths); the merge
    /// or rebase is left in progress for the user to resolve.
    #[error("conflicts in {} files", paths.len())]
    Conflicts { paths: Vec<PathBuf> },
    /// A fast-forward-only pull found local commits the upstream doesn't have.
    /// Nothing was changed.
    #[error("not possible to fast-forward")]
    NotFastForward,
    /// The remote refused the push: it has commits the local branch doesn't
    /// (or a lease went stale). `output` is all git printed.
    #[error("push rejected:\n{output}")]
    Rejected { output: String },
    /// The remote wanted credentials git couldn't get without a prompt, or
    /// refused them. `output` is all git printed.
    #[error("authentication failed:\n{output}")]
    Auth { output: String },
    /// Git failed for another reason, and `output` is all it printed; or it
    /// wasn't run because it would have signed, and `output` says so.
    #[error("git failed:\n{output}")]
    Failed { output: String },
    #[error(transparent)]
    Git(#[from] GitError),
}

/// What git prints when a remote wants credentials it can't get or refuses
/// them.
const AUTH_MARKERS: &[&str] = &[
    "Authentication failed",
    "could not read Username",
    "could not read Password",
    "Permission denied (",
];

/// Every signer git could run, `false`, given as `-c` options, which git hands
/// on to the git commands it runs in submodules.
const NO_SIGNERS: [&str; 8] = [
    "-c",
    "gpg.program=false",
    "-c",
    "gpg.openpgp.program=false",
    "-c",
    "gpg.x509.program=false",
    "-c",
    "gpg.ssh.program=false",
];

/// Why a pull or push that would sign wasn't run.
const SIGNING_REFUSED: &str = "signing is on and could ask for a passphrase, so this wasn't done; \
run it from a terminal";

impl Repo {
    /// Fetches HEAD's upstream remote, or `origin` when HEAD's branch has none
    /// or HEAD is detached, deleting remote-tracking branches gone from the
    /// remote when `prune` (otherwise `fetch.prune` decides). `FETCH_HEAD`
    /// isn't written.
    pub fn fetch(&self, prune: bool) -> Result<(), RemoteError> {
        let upstream = match self.branch()? {
            Some(branch) => self.upstream_of(&branch)?,
            None => None,
        };
        let remote = match upstream {
            Some((remote, _)) => remote,
            None if self.remotes()?.iter().any(|name| name == "origin") => "origin".to_owned(),
            None => return Err(RemoteError::NoRemote),
        };
        let mut args: Vec<OsString> = ["fetch", "-q", "--no-write-fetch-head"]
            .map(OsString::from)
            .into();
        if prune {
            args.push("--prune".into());
        }
        args.extend(["--".into(), remote.into()]);
        let output = self.run_remote(&args)?;
        outcome(output)
    }

    /// Fetches HEAD's upstream and brings its commits into HEAD's branch as
    /// `mode` says. Local changes are never stashed: those the pull would
    /// overwrite stop it, with [`RemoteError::Failed`]. Submodules aren't
    /// updated. When the merge or rebase would sign its commits, one that needs
    /// a new commit is [`RemoteError::Failed`] with nothing changed; a
    /// fast-forward goes ahead.
    pub fn pull(&self, mode: PullMode) -> Result<(), RemoteError> {
        let branch = self.branch()?.ok_or(RemoteError::Detached)?;
        if self.upstream_of(&branch)?.is_none() {
            return Err(RemoteError::NoUpstream);
        }
        let signs = mode != PullMode::FastForwardOnly && self.signs_commits(&branch)?;
        let mut args: Vec<OsString> = [
            "pull",
            "-q",
            "--no-edit",
            "--no-autostash",
            "--no-recurse-submodules",
        ]
        .map(OsString::from)
        .into();
        let flags: &[&str] = match mode {
            _ if signs => &["--no-rebase", "--ff-only"],
            PullMode::Merge => &["--no-rebase", "--ff"],
            PullMode::Rebase => &["--rebase"],
            PullMode::FastForwardOnly => &["--no-rebase", "--ff-only"],
        };
        args.extend(flags.iter().map(OsString::from));
        let output = self.run_remote(&args)?;
        // Checked on success too, in case something left the merge half done.
        let paths = self.unmerged_paths()?;
        if !paths.is_empty() {
            return Err(RemoteError::Conflicts { paths });
        }
        if !output.status.success()
            && String::from_utf8_lossy(&output.stderr).contains("Not possible to fast-forward")
        {
            if signs {
                return Err(RemoteError::Failed {
                    output: format!("commit signing: {SIGNING_REFUSED}"),
                });
            }
            if mode == PullMode::FastForwardOnly {
                return Err(RemoteError::NotFastForward);
            }
        }
        outcome(output)
    }

    /// Pushes HEAD's branch to its upstream branch, with `--force-with-lease`
    /// when `force_with_lease` (never a plain `--force`): the remote branch is
    /// replaced only if it's still where the last fetch saw it.
    pub fn push(&self, force_with_lease: bool) -> Result<(), RemoteError> {
        let Some((remote, merge)) = self.upstream()? else {
            return Err(RemoteError::NoUpstream);
        };
        let mut args = self.push_args()?;
        if force_with_lease {
            args.push("--force-with-lease".into());
        }
        args.extend(["--".into(), remote.into(), format!("HEAD:{merge}").into()]);
        let output = self.run_remote(&args)?;
        outcome(output)
    }

    /// Pushes HEAD's branch to a branch of the same name on `remote` and makes
    /// that its upstream (`git push -u <remote> HEAD`).
    pub fn publish(&self, remote: &str) -> Result<(), RemoteError> {
        self.branch()?.ok_or(RemoteError::Detached)?;
        let mut args = self.push_args()?;
        args.extend(["-u", "--", remote, "HEAD"].map(OsString::from));
        let output = self.run_remote(&args)?;
        outcome(output)
    }

    /// Whether a merge or rebase on `branch` signs its commits:
    /// `commit.gpgSign`, unless `branch.<name>.mergeOptions` says otherwise.
    fn signs_commits(&self, branch: &str) -> Result<bool, GitError> {
        let mut signs = self.config_bool("commit.gpgSign")?;
        let options = self.config(&format!("branch.{branch}.mergeOptions"))?;
        for option in options.as_deref().unwrap_or("").split_whitespace() {
            if option.starts_with("-S") || option.starts_with("--gpg-sign") {
                signs = true;
            } else if option == "--no-gpg-sign" {
                signs = false;
            }
        }
        Ok(signs)
    }

    /// `git push` and its options shared by push and publish. `push.gpgSign`
    /// true, as git reads a boolean, is [`RemoteError::Failed`]; `if-asked`
    /// pushes unsigned (`--no-signed`), as it would to a server that doesn't
    /// ask.
    fn push_args(&self) -> Result<Vec<OsString>, RemoteError> {
        let signs = match self.config_get(&["config", "--get", "push.gpgSign"])? {
            None => false,
            Some(value) if value.eq_ignore_ascii_case("if-asked") => false,
            Some(_) => self.config_bool("push.gpgSign")?,
        };
        if signs {
            return Err(RemoteError::Failed {
                output: format!("push.gpgSign: {SIGNING_REFUSED}"),
            });
        }
        Ok(["push", "-q", "--no-signed"].map(OsString::from).into())
    }

    /// HEAD's branch's upstream remote and the branch's full name there
    /// (`refs/heads/main`); `None` when it has none. Detached HEAD is
    /// [`RemoteError::Detached`].
    fn upstream(&self) -> Result<Option<(String, String)>, RemoteError> {
        let branch = self.branch()?.ok_or(RemoteError::Detached)?;
        Ok(self.upstream_of(&branch)?)
    }

    fn upstream_of(&self, branch: &str) -> Result<Option<(String, String)>, GitError> {
        let remote = self.config(&format!("branch.{branch}.remote"))?;
        let merge = self.config(&format!("branch.{branch}.merge"))?;
        Ok(remote.zip(merge))
    }

    /// HEAD's branch by its short name, `None` when HEAD is detached.
    fn branch(&self) -> Result<Option<String>, GitError> {
        let args = ["symbolic-ref", "-q", "HEAD"].map(OsString::from);
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        // Exit 1: HEAD isn't a symbolic ref.
        match output.status.code() {
            Some(0) => Ok(non_empty(&output.stdout).map(|name| {
                name.strip_prefix("refs/heads/")
                    .map_or(name.clone(), str::to_owned)
            })),
            Some(1) => Ok(None),
            _ => Err(command_error(&args, &output)),
        }
    }

    /// The value of config `key`, `None` when unset or empty.
    fn config(&self, key: &str) -> Result<Option<String>, GitError> {
        Ok(self
            .config_get(&["config", "--get", key])?
            .filter(|value| !value.is_empty()))
    }

    /// Whether boolean config `key` is set and true.
    fn config_bool(&self, key: &str) -> Result<bool, GitError> {
        let value = self.config_get(&["config", "--type=bool", "--get", key])?;
        Ok(value.as_deref() == Some("true"))
    }

    /// What a `git config --get` prints, trimmed (empty for a key with no
    /// value); `None` when the key isn't set.
    fn config_get(&self, args: &[&str]) -> Result<Option<String>, GitError> {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let output = run_raw(&self.workdir, Access::Read, &args, None, &[])?;
        // Exit 1: the key isn't set.
        match output.status.code() {
            Some(0) => Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            )),
            Some(1) => Ok(None),
            _ => Err(command_error(&args, &output)),
        }
    }

    fn remotes(&self) -> Result<Vec<String>, GitError> {
        let stdout = run(&self.workdir, Access::Read, &["remote".into()], None)?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// The unmerged files, as absolute paths.
    fn unmerged_paths(&self) -> Result<Vec<PathBuf>, GitError> {
        let args = [
            "diff",
            "--name-only",
            "--no-relative",
            "-z",
            "--diff-filter=U",
        ]
        .map(OsString::from);
        let stdout = run(&self.workdir, Access::Read, &args, None)?;
        stdout
            .split(|&byte| byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| {
                let path = gix::path::from_byte_slice(name).map_err(GitError::from_gix)?;
                Ok(self.workdir.join(path))
            })
            .collect()
    }

    /// Runs a git command that talks to a remote, with every prompt off.
    fn run_remote(&self, args: &[OsString]) -> Result<Output, GitError> {
        // An empty GIT_ASKPASS also keeps git from falling back to SSH_ASKPASS
        // for credentials; Git Credential Manager has its own switch.
        let mut envs: Vec<(&str, &Path)> = vec![
            ("GIT_ASKPASS", "".as_ref()),
            ("GCM_INTERACTIVE", "never".as_ref()),
            ("GIT_EDITOR", "true".as_ref()),
        ];
        let own_ssh = ["GIT_SSH_COMMAND", "GIT_SSH"]
            .iter()
            .any(|var| std::env::var_os(var).is_some())
            || self.config("core.sshCommand")?.is_some();
        if !own_ssh {
            envs.push(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes".as_ref()));
        }
        let args: Vec<OsString> = NO_SIGNERS
            .iter()
            .map(OsString::from)
            .chain(args.iter().cloned())
            .collect();
        run_raw(&self.workdir, Access::Write, &args, None, &envs)
    }
}

/// Trimmed stdout, `None` when that's empty.
fn non_empty(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn command_error(args: &[OsString], output: &Output) -> GitError {
    GitError::Command {
        args: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

/// `Ok` for a git that succeeded, else the error its output names.
fn outcome(output: Output) -> Result<(), RemoteError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&stderr);
    let output = text.trim().to_owned();
    Err(
        if stderr.contains("[rejected]") || stderr.contains("[remote rejected]") {
            RemoteError::Rejected { output }
        } else if AUTH_MARKERS.iter().any(|marker| stderr.contains(marker)) {
            RemoteError::Auth { output }
        } else {
            RemoteError::Failed { output }
        },
    )
}
