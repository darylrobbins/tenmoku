//! The launch seam (docs/decisions/lsp-client.md §3.1): fe-lsp never spawns
//! a process itself. It asks a [`ServerLauncher`] for a server's byte
//! streams, on the server's own thread.

use std::future::Future;
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;

use futures::{AsyncRead, AsyncWrite};

/// What to start: from settings or supplied by the host.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerSpec {
    /// The server's name, e.g. "rust-analyzer". Names its thread and its
    /// status.
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Set on top of the launcher's own environment.
    pub env: Vec<(String, String)>,
    /// fe-syntax language names the server handles.
    pub languages: Vec<String>,
    /// Sent as `initializationOptions`.
    pub initialization_options: Option<serde_json::Value>,
    /// Answers `workspace/configuration`: each item gets the value at its
    /// dotted `section` within this, or `null`.
    pub settings: serde_json::Value,
    pub readiness: ReadinessMode,
    /// Work-done progress token prefixes that don't hold back readiness.
    pub ready_ignores_progress: Vec<String>,
    pub unversioned_diagnostics: UnversionedDiagnostics,
}

impl ServerSpec {
    /// A spec running `command` with no arguments, environment, languages
    /// or settings, and the default modes.
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
            languages: Vec::new(),
            initialization_options: None,
            settings: serde_json::Value::Null,
            readiness: ReadinessMode::default(),
            ready_ignores_progress: Vec::new(),
            unversioned_diagnostics: UnversionedDiagnostics::default(),
        }
    }
}

/// How a server's readiness is decided (§3.4). Chosen before startup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadinessMode {
    /// rust-analyzer's `experimental/serverStatus` with `quiescent: true`.
    ServerStatus,
    /// `initialize` has answered and no work-done progress has been open
    /// for 500 ms.
    #[default]
    Progress,
}

/// What happens to a `publishDiagnostics` without a version (§4.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnversionedDiagnostics {
    /// Dropped: nothing fixes the text they were computed on.
    #[default]
    Drop,
}

/// A running server's transport. Client → server bytes go to `input`.
pub struct ServerIo {
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
    pub output: Pin<Box<dyn AsyncRead + Send>>,
    pub stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    pub process: Box<dyn ServerProcess>,
}

/// How a server went away. Both fields are `None` when only its transport
/// is known to have closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// The launcher's handle on what it started, for shutdown and crash
/// detection.
pub trait ServerProcess: Send {
    /// Resolves when the server has gone: exited, or its transport closed.
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>>;
    /// Forcibly stops it (kill the process group, close the proxy …).
    fn kill(&mut self);
}

/// Hands fe-lsp a server's byte streams. A host implements it to sandbox,
/// containerise or proxy servers, or to refuse one: an `Err` marks the
/// server `Stopped` with the error's message.
pub trait ServerLauncher: Send + Sync {
    /// Called on the server's own thread; may block (e.g. start a
    /// container).
    fn launch(&self, spec: &ServerSpec, root: &Path) -> io::Result<ServerIo>;
}

/// The default launcher: a local process in its own process group, with
/// piped stdio, cwd = root, the spec's environment and `kill_on_drop`.
/// [`ServerProcess::kill`] signals the whole group, so the server's own
/// children go too.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalLauncher;

impl ServerLauncher for LocalLauncher {
    fn launch(&self, spec: &ServerSpec, root: &Path) -> io::Result<ServerIo> {
        let mut command = std::process::Command::new(&spec.command);
        command
            .args(&spec.args)
            .current_dir(root)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .process_group(0);
        let mut command = async_process::Command::from(command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let (Some(input), Some(output), stderr) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(io::Error::other("the child's stdio wasn't piped"));
        };
        Ok(ServerIo {
            input: Box::pin(input),
            output: Box::pin(output),
            stderr: stderr.map(|s| Box::pin(s) as Pin<Box<dyn AsyncRead + Send>>),
            process: Box::new(LocalProcess { child, exit: None }),
        })
    }
}

struct LocalProcess {
    child: async_process::Child,
    exit: Option<ExitInfo>,
}

impl ServerProcess for LocalProcess {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(async move {
            if let Some(exit) = self.exit {
                return exit;
            }
            let exit = match self.child.status().await {
                Ok(status) => ExitInfo {
                    code: status.code(),
                    signal: status.signal(),
                },
                Err(err) => {
                    log::warn!("waiting for language server {}: {err}", self.child.id());
                    ExitInfo::default()
                }
            };
            self.exit = Some(exit);
            exit
        })
    }

    fn kill(&mut self) {
        // The child leads its group (`process_group(0)`), so the group id is
        // its pid. ESRCH means the whole group has already gone. Signalled
        // even after the leader was reaped, so children it left behind go
        // too: a pid isn't reused while its group still has members.
        let group = i32::try_from(self.child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw);
        if let Some(group) = group
            && let Err(err) =
                rustix::process::kill_process_group(group, rustix::process::Signal::KILL)
            && err != rustix::io::Errno::SRCH
        {
            log::warn!("killing language server group {}: {err}", self.child.id());
        }
        let _ = self.child.kill();
    }
}
