//! The `git` executable, the one way anything here writes to a repository.
//!
//! gix doesn't apply clean filters or autocrlf when staging, so writes run
//! `git` itself, as VS Code does. Every call blocks until git exits; callers
//! run it off the UI thread.

use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::GitError;

/// What a call to git is for: reads leave the index alone, so they must not
/// take its optional locks and race a write running beside them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Write,
}

/// Runs `git -C <workdir> <args>`, with `stdin` piped in when given, and
/// returns its stdout. A missing `git` is [`GitError::GitMissing`]; a non-zero
/// exit is [`GitError::Command`].
pub(crate) fn run(
    workdir: &Path,
    access: Access,
    args: &[OsString],
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, GitError> {
    let mut command = Command::new("git");
    // Paths are literal file names, never pathspec patterns.
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(workdir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if access == Access::Read {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }
    let mut child = command.spawn().map_err(|err| match err.kind() {
        ErrorKind::NotFound => GitError::GitMissing,
        _ => GitError::Io(err.to_string()),
    })?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A git that exits early closes the pipe; its status says why.
        match pipe.write_all(input) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::BrokenPipe => {}
            Err(err) => return Err(GitError::Io(err.to_string())),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|err| GitError::Io(err.to_string()))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(GitError::Command {
        args: args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}
