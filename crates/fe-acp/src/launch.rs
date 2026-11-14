//! Launching the Claude Agent adapter (decision record § 4, "Locating the
//! programs" and "Session lifecycle").
//!
//! The workbench resolves both programs on the login `PATH` (or from the
//! two path settings) and hands fe-acp plain paths. [`launch`] spawns the
//! adapter as `launch_local` spawns a language server:
//!
//! - the opened root as its working directory (sessions set their own
//!   `cwd`);
//! - its own process group, killed together on shutdown or drop;
//! - stdin and stdout as the ACP transport, and stderr kept in a ring of
//!   its last lines, for the "Disconnected" state's details;
//! - the environment inherited, with the login `PATH`, plus
//!   `CLAUDE_CODE_EXECUTABLE` (so the adapter runs the user's own `claude`,
//!   not the SDK's copy) and `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` (so
//!   Claude Code's Bash runs nothing in the background, which would outlive
//!   a cancel and a freeze: F10). Nothing else is added.
//!
//! Every spawn and every respawn goes through [`launch`], so each gets the
//! same environment.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use agent_client_protocol::ByteStreams;
use agent_client_protocol::v1::schema::SessionId;
use futures::FutureExt as _;
use futures::channel::mpsc;
use futures::future::BoxFuture;
use futures::io::{AsyncReadExt as _, AsyncWrite};

use crate::claude::AgentState;
use crate::{AcpClient, Incoming};

/// The variable that tells the adapter which `claude` to run.
pub const CLAUDE_CODE_EXECUTABLE: &str = "CLAUDE_CODE_EXECUTABLE";

/// The variable that stops Claude Code running commands in the background
/// (F10). Set to `1` on every spawn.
pub const DISABLE_BACKGROUND_TASKS: &str = "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS";

/// The adapter's flag that hides the claude.ai login and refuses every
/// session and turn a subscription would pay for (open question 1).
pub const HIDE_CLAUDE_AUTH: &str = "--hide-claude-auth";

/// How long shutdown waits for the adapter to exit after its stdin closes,
/// before killing its process group.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// How many of the adapter's last stderr lines are kept.
pub const STDERR_LINES: usize = 200;

/// The longest stderr line kept, in bytes; longer ones are cut.
const STDERR_LINE_MAX: usize = 4096;

/// What to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The adapter, `claude-agent-acp`: `None` when it wasn't found.
    pub adapter: Option<PathBuf>,
    /// `claude`: `None` when it wasn't found.
    pub claude: Option<PathBuf>,
    /// The opened root: the adapter's working directory.
    pub root: PathBuf,
    /// The login shell's `PATH`, if resolved.
    pub path: Option<OsString>,
    /// Starts the adapter with [`HIDE_CLAUDE_AUTH`].
    pub hide_claude_auth: bool,
    /// Arguments after the adapter's own (the fake agent's script, in
    /// tests).
    pub args: Vec<OsString>,
}

impl LaunchSpec {
    /// The adapter at `adapter`, running `claude`, in `root`.
    #[must_use]
    pub fn new(
        adapter: Option<PathBuf>,
        claude: Option<PathBuf>,
        root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            adapter,
            claude,
            root: root.into(),
            path: None,
            hide_claude_auth: false,
            args: Vec::new(),
        }
    }
}

/// A launched adapter: the ACP client over its stdio, the stream of what
/// it sends, the connection future for the caller to drive, and its
/// process.
pub struct Launched {
    /// The client.
    pub client: AcpClient,
    /// What the adapter sends.
    pub incoming: mpsc::UnboundedReceiver<Incoming>,
    /// The connection, to drive on a background executor. It ends when the
    /// adapter's stdout closes or the client shuts down.
    pub connection: BoxFuture<'static, crate::Result<()>>,
    /// The process.
    pub process: AgentProcess,
}

impl std::fmt::Debug for Launched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launched")
            .field("process", &self.process)
            .finish_non_exhaustive()
    }
}

/// Spawns the adapter of `spec` and connects to it. No request is sent.
///
/// # Errors
///
/// [`AgentState::AdapterNotInstalled`] or [`AgentState::ClaudeNotInstalled`]
/// when a program is missing, and [`AgentState::Failed`] when the spawn
/// fails otherwise.
pub fn launch(spec: &LaunchSpec) -> Result<Launched, AgentState> {
    let adapter = spec
        .adapter
        .as_deref()
        .filter(|path| is_program(path))
        .ok_or(AgentState::AdapterNotInstalled)?;
    let claude = spec
        .claude
        .as_deref()
        .filter(|path| is_program(path))
        .ok_or(AgentState::ClaudeNotInstalled)?;
    let mut command = std::process::Command::new(adapter);
    if let Some(path) = &spec.path {
        command.env("PATH", path);
    }
    if spec.hide_claude_auth {
        command.arg(HIDE_CLAUDE_AUTH);
    }
    command
        .args(&spec.args)
        .current_dir(&spec.root)
        .env(CLAUDE_CODE_EXECUTABLE, claude)
        .env(DISABLE_BACKGROUND_TASKS, "1");
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut command = async_process::Command::from(command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => AgentState::AdapterNotInstalled,
        _ => AgentState::Failed {
            details: format!("starting {} failed: {err}", adapter.display()),
        },
    })?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(AgentState::Failed {
            details: "the adapter's stdio wasn't piped".into(),
        });
    };
    let stdin = SharedStdin(Arc::new(Mutex::new(Some(stdin))));
    let process = AgentProcess {
        inner: Arc::new(ProcessInner {
            id: child.id(),
            child: Mutex::new(child),
            stdin: stdin.clone(),
            stderr: Arc::new(Mutex::new(VecDeque::new())),
            stderr_done: Arc::new(AtomicBool::new(false)),
        }),
    };
    process.read_stderr(stderr);
    let (client, incoming, connection) = crate::connect(ByteStreams::new(stdin, stdout));
    Ok(Launched {
        client,
        incoming,
        connection: connection.boxed(),
        process,
    })
}

/// Whether `path` is a file to run (an executable regular file, links
/// followed, on Unix).
fn is_program(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// The adapter's stdin, which shutdown closes whether or not the
/// connection future is still being driven.
#[derive(Clone)]
struct SharedStdin(Arc<Mutex<Option<async_process::ChildStdin>>>);

impl SharedStdin {
    fn close(&self) {
        lock(&self.0).take();
    }

    fn with<T>(
        &self,
        f: impl FnOnce(Pin<&mut async_process::ChildStdin>) -> Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        match lock(&self.0).as_mut() {
            Some(stdin) => f(Pin::new(stdin)),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }
}

impl AsyncWrite for SharedStdin {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.with(|stdin| stdin.poll_write(cx, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.with(|stdin| stdin.poll_flush(cx))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let closed = self.with(|stdin| stdin.poll_close(cx));
        if closed.is_ready() {
            self.close();
        }
        match closed {
            Poll::Ready(Err(err)) if err.kind() == io::ErrorKind::BrokenPipe => Poll::Ready(Ok(())),
            other => other,
        }
    }
}

/// The adapter's process. Clones share it; when the last is dropped, its
/// process group is killed.
#[derive(Clone)]
pub struct AgentProcess {
    inner: Arc<ProcessInner>,
}

struct ProcessInner {
    id: u32,
    child: Mutex<async_process::Child>,
    stdin: SharedStdin,
    stderr: Arc<Mutex<VecDeque<String>>>,
    stderr_done: Arc<AtomicBool>,
}

impl std::fmt::Debug for AgentProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProcess")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}

impl AgentProcess {
    /// Its process id, which is also its process group's.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.inner.id
    }

    /// The last [`STDERR_LINES`] lines it wrote to stderr, oldest first.
    #[must_use]
    pub fn stderr_tail(&self) -> Vec<String> {
        lock(&self.inner.stderr).iter().cloned().collect()
    }

    /// Its exit status, if it has exited.
    #[must_use]
    pub fn try_exited(&self) -> Option<ExitStatus> {
        lock(&self.inner.child).try_status().ok().flatten()
    }

    /// Waits up to `timeout` for it to exit.
    pub async fn exited(&self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_exited() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    }

    /// Waits up to `timeout` for its stderr to end, so the tail holds what
    /// it wrote last.
    pub async fn stderr_finished(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.inner.stderr_done.load(Ordering::Acquire) {
            if Instant::now() >= deadline {
                return false;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        true
    }

    /// Closes its stdin.
    pub fn close_stdin(&self) {
        self.inner.stdin.close();
    }

    /// Kills its process group, and it.
    pub fn kill(&self) {
        self.inner.kill();
    }

    /// Shuts it down (§ 4's "Shutdown"): `session/close` for each of
    /// `sessions`, within [`SHUTDOWN_GRACE`]; then the connection ends and
    /// stdin closes; then, once it exits or [`SHUTDOWN_GRACE`] passes, its
    /// process group is killed, taking whatever it started with it.
    pub async fn shutdown(&self, client: &AcpClient, sessions: &[SessionId]) {
        let close = async {
            for session_id in sessions {
                if let Err(err) = client.close_session(session_id.clone()).await {
                    log::debug!("fe-acp: closing session {session_id} failed: {err}");
                }
            }
        };
        let timeout = async_io::Timer::after(SHUTDOWN_GRACE);
        futures::pin_mut!(close);
        futures::future::select(close, timeout).await;
        client.shutdown();
        self.close_stdin();
        self.exited(SHUTDOWN_GRACE).await;
        self.kill();
        // Reap it.
        self.exited(SHUTDOWN_GRACE).await;
    }

    /// Drains stderr on a thread of its own, in bounded chunks: each line
    /// keeps at most [`STDERR_LINE_MAX`] bytes (the rest is read and
    /// dropped, however long the line runs without a newline), and is
    /// decoded lossily, so bytes that aren't UTF-8 never stop the reading.
    fn read_stderr(&self, mut stderr: async_process::ChildStderr) {
        let ring = self.inner.stderr.clone();
        let done = self.inner.stderr_done.clone();
        let spawned = std::thread::Builder::new()
            .name("fe-acp stderr".into())
            .spawn(move || {
                async_io::block_on(async {
                    let mut lines = StderrLines {
                        ring,
                        line: Vec::new(),
                    };
                    let mut chunk = [0u8; 4096];
                    loop {
                        match stderr.read(&mut chunk).await {
                            Ok(0) => break,
                            Ok(read) => lines.push(&chunk[..read]),
                            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                            Err(err) => {
                                log::debug!("fe-acp: reading the adapter's stderr: {err}");
                                break;
                            }
                        }
                    }
                    // A last line with no newline.
                    if !lines.line.is_empty() {
                        lines.finish();
                    }
                });
                done.store(true, Ordering::Release);
            });
        if let Err(err) = spawned {
            log::warn!("fe-acp: no thread to read the adapter's stderr: {err}");
            self.inner.stderr_done.store(true, Ordering::Release);
        }
    }
}

/// Splits stderr's bytes into the ring's lines.
struct StderrLines {
    ring: Arc<Mutex<VecDeque<String>>>,
    /// The line so far, at most [`STDERR_LINE_MAX`] bytes of it.
    line: Vec<u8>,
}

impl StderrLines {
    fn push(&mut self, mut bytes: &[u8]) {
        while let Some(end) = bytes.iter().position(|&b| b == b'\n') {
            self.keep(&bytes[..end]);
            self.finish();
            bytes = &bytes[end + 1..];
        }
        self.keep(bytes);
    }

    fn keep(&mut self, bytes: &[u8]) {
        let room = STDERR_LINE_MAX.saturating_sub(self.line.len());
        self.line.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }

    /// Ends the line so far: at a newline, or at the end.
    fn finish(&mut self) {
        let bytes = std::mem::take(&mut self.line);
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(&bytes);
        let mut line = String::from_utf8_lossy(bytes).into_owned();
        if line.len() > STDERR_LINE_MAX {
            let mut end = STDERR_LINE_MAX;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        let mut ring = lock(&self.ring);
        if ring.len() == STDERR_LINES {
            ring.pop_front();
        }
        ring.push_back(line);
    }
}

impl ProcessInner {
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(group) = i32::try_from(self.id)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            && let Err(err) =
                rustix::process::kill_process_group(group, rustix::process::Signal::KILL)
            && err != rustix::io::Errno::SRCH
        {
            log::warn!(
                "fe-acp: killing the adapter's process group {}: {err}",
                self.id
            );
        }
        let _ = lock(&self.child).kill();
    }
}

impl Drop for ProcessInner {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Locks, ignoring poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
