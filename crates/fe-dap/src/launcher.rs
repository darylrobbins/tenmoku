// SPDX-License-Identifier: Apache-2.0

//! The launch seam (docs/decisions/r1b9dw3g0g-debugger.md §2), mirroring
//! fe-lsp's `ServerLauncher`: fe-dap never spawns an adapter itself. It asks
//! an [`AdapterLauncher`] for the adapter's byte streams, on the session's
//! own thread, so a host can sandbox or refuse adapters.
//!
//! [`LocalLauncher`] is the default: a local process in its own process
//! group. Over [`AdapterTransport::Tcp`] it picks a free port, puts it in
//! the arguments for `${port}`, and connects to `127.0.0.1` on it, retrying
//! for up to [`CONNECT_TIMEOUT`] on its [`Clock`]. Over
//! [`AdapterTransport::Port`] it starts nothing: it connects, the same way,
//! to the port of an adapter the host has already started (§13: jdtls's
//! java-debug).

use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::{Path, PathBuf};
use std::pin::{Pin, pin};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_io::Async;
use futures::channel::oneshot;
use futures::future::{Either, FutureExt as _, Shared};
use futures::{AsyncRead, AsyncReadExt as _, AsyncWrite};
use kiln_l10n::tr;

use crate::clock::{Clock, SystemClock};
use crate::protocol::Object;

/// What [`AdapterTransport::Tcp`] replaces with the port, in each argument.
pub const PORT_PLACEHOLDER: &str = "${port}";

/// How long [`LocalLauncher`] keeps trying to connect to a TCP adapter.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long it waits between connection attempts.
pub const CONNECT_RETRY: Duration = Duration::from_millis(100);

/// What to start (§3): from the registry, settings, an extension, or the
/// host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdapterSpec {
    /// The adapter's name, e.g. "debugpy". Names its session's thread.
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Set on top of the launcher's own environment.
    pub env: Vec<(String, String)>,
    pub transport: AdapterTransport,
    /// fe-syntax language names it debugs, for picking a default.
    pub languages: Vec<String>,
    /// The VS Code `type` strings it answers to (`launch.json`'s `type`).
    pub types: Vec<String>,
    /// Merged under each launch configuration's arguments.
    pub launch_defaults: Object,
    /// LLDB commands run at startup (an LLDB adapter's `initCommands`).
    pub init_commands: Vec<String>,
}

impl AdapterSpec {
    /// A stdio spec running `command` with no arguments or environment.
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            ..Self::default()
        }
    }
}

/// How the client talks to an adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterTransport {
    /// Over the adapter's stdin and stdout.
    #[default]
    Stdio,
    /// Over a TCP connection to `127.0.0.1`, on the port the launcher puts
    /// in the arguments for [`PORT_PLACEHOLDER`].
    Tcp,
    /// Over a TCP connection to `127.0.0.1` on this port, where an adapter
    /// the host has already started listens (§13: the port jdtls gives for
    /// java-debug). Nothing is launched, so `command`, `args` and `env` are
    /// unused, and the adapter's process is the host's to stop.
    Port(u16),
}

/// A running adapter's transport. Client → adapter bytes go to `input`.
pub struct AdapterIo {
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
    pub output: Pin<Box<dyn AsyncRead + Send>>,
    pub stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    /// The adapter's stdout when it isn't the transport (over TCP), for the
    /// session to drain and log.
    pub stdout: Option<Pin<Box<dyn AsyncRead + Send>>>,
    /// The port connected to over TCP.
    pub port: Option<u16>,
    pub process: Box<dyn AdapterProcess>,
}

/// How an adapter went away. Both fields are `None` when only its transport
/// is known to have closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// The launcher's handle on what it started, for shutdown and crash
/// detection.
pub trait AdapterProcess: Send {
    /// The process id, if there is a process.
    fn id(&self) -> Option<u32>;
    /// Resolves when the adapter has gone: exited, or its transport closed.
    /// Also after [`kill`](Self::kill), once it has taken effect.
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>>;
    /// Forcibly stops it: its whole process group.
    fn kill(&mut self);
}

/// Hands fe-dap an adapter's byte streams. A host implements it to sandbox,
/// proxy or refuse adapters: an `Err` fails the session's launch with the
/// error's message.
pub trait AdapterLauncher: Send + Sync {
    /// Called on the session's own thread, with the debuggee's working
    /// directory; may block.
    ///
    /// `cancel` fires when the session is stopped while it launches. A
    /// launch should then return promptly, with
    /// [`ErrorKind::Interrupted`](io::ErrorKind::Interrupted), having
    /// stopped what it started.
    fn launch(
        &self,
        spec: &AdapterSpec,
        cwd: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<AdapterIo>;
}

/// Asks an [`AdapterLauncher::launch`] in progress to give up. Clones share
/// it, and once fired it stays fired.
#[derive(Clone, Default)]
pub struct LaunchCancel {
    flag: Arc<Flag>,
}

impl LaunchCancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.fire();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.fired()
    }

    /// Resolves once it's cancelled, at once if it is already.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static {
        let flag = self.flag.clone();
        async move {
            let _ = flag.receiver.clone().await;
        }
    }
}

impl std::fmt::Debug for LaunchCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchCancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// A one-way flag that can be waited on.
struct Flag {
    fired: AtomicBool,
    /// Taken (dropped) when it fires, which resolves `receiver`.
    sender: Mutex<Option<oneshot::Sender<()>>>,
    receiver: Shared<oneshot::Receiver<()>>,
}

impl Default for Flag {
    fn default() -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            fired: AtomicBool::new(false),
            sender: Mutex::new(Some(sender)),
            receiver: receiver.shared(),
        }
    }
}

impl Flag {
    fn fire(&self) {
        self.fired.store(true, Ordering::SeqCst);
        drop(
            self.sender
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

/// The default launcher: a local process in its own process group (on
/// Unix), with the spec's environment, working in `cwd`, killed with its
/// group when its handle is dropped unseen to exit.
///
/// - A bare `command` (no `/`) is looked up on the `PATH` given to
///   [`with_path`](Self::with_path) (the login shell's, which the host
///   resolves as fe-lsp does), else on this process's, using absolute
///   entries only: a `.` or empty entry would resolve under `cwd`, which a
///   cloned repository may control. The adapter gets that `PATH` too,
///   unless the spec's `env` sets one, which is then also the one searched.
/// - A relative command with a `/` is under `cwd`; an absolute one is used
///   as it is.
/// - Over [`AdapterTransport::Tcp`], stdin is closed, stdout is handed on as
///   [`AdapterIo::stdout`], and the port is one the system just gave out
///   free. Another program could take it between that and the adapter's
///   bind, which shows as a failed connection.
/// - Over [`AdapterTransport::Port`], nothing is started: it connects to
///   the host's adapter on that port, with the same retries and limit.
#[derive(Clone)]
pub struct LocalLauncher {
    clock: Arc<dyn Clock>,
    path: Option<OsString>,
    /// How long a TCP adapter has to start listening.
    connect_timeout: Duration,
}

impl Default for LocalLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LocalLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalLauncher")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl LocalLauncher {
    /// On real time, looking bare commands up on this process's `PATH`.
    pub fn new() -> Self {
        Self {
            clock: Arc::new(SystemClock),
            path: None,
            connect_timeout: CONNECT_TIMEOUT,
        }
    }

    /// Giving a TCP adapter `timeout` to start listening, not
    /// [`CONNECT_TIMEOUT`] (the user's `debug.timeouts.connect`).
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Measuring its connection retries on `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Looking bare commands up on `path`, and giving it to the adapter.
    pub fn with_path(mut self, path: Option<OsString>) -> Self {
        self.path = path;
        self
    }
}

impl AdapterLauncher for LocalLauncher {
    fn launch(
        &self,
        spec: &AdapterSpec,
        cwd: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<AdapterIo> {
        if cancel.is_cancelled() {
            return Err(cancelled(spec));
        }
        if let AdapterTransport::Port(port) = spec.transport {
            return self.connect_to(spec, port, cancel);
        }
        let spec_path = spec
            .env
            .iter()
            .rev()
            .find(|(name, _)| name == "PATH")
            .map(|(_, path)| OsString::from(path));
        let search = spec_path.clone().or_else(|| self.path.clone());
        let program = resolve_program(&spec.command, search.as_deref(), cwd).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                tr!(
                    "dap-adapter-not-found",
                    name = spec.command.display().to_string()
                )
                .to_string(),
            )
        })?;
        let (args, port) = match spec.transport {
            AdapterTransport::Stdio => (spec.args.clone(), None),
            AdapterTransport::Port(_) => unreachable!("connected to above"),
            AdapterTransport::Tcp => {
                if !spec.args.iter().any(|arg| arg.contains(PORT_PLACEHOLDER)) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        tr!("dap-launch-no-port", name = spec.name.as_str()).to_string(),
                    ));
                }
                let port = free_port()?;
                let args = spec
                    .args
                    .iter()
                    .map(|arg| arg.replace(PORT_PLACEHOLDER, &port.to_string()))
                    .collect();
                (args, Some(port))
            }
        };
        let mut command = std::process::Command::new(program);
        if spec_path.is_none()
            && let Some(path) = &self.path
        {
            command.env("PATH", path);
        }
        command
            .args(&args)
            .current_dir(cwd)
            .envs(spec.env.iter().map(|(k, v)| (k, v)));
        // The adapter and the debuggee it starts are killed together.
        #[cfg(unix)]
        command.process_group(0);
        let mut command = async_process::Command::from(command);
        command
            .stdin(if port.is_some() {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child
            .stderr
            .take()
            .map(|s| Box::pin(s) as Pin<Box<dyn AsyncRead + Send>>);
        let mut process = LocalProcess { child, exit: None };
        let Some(port) = port else {
            let (Some(input), Some(output)) = (stdin, stdout) else {
                return Err(io::Error::other(tr!("dap-launch-stdio").to_string()));
            };
            return Ok(AdapterIo {
                input: Box::pin(input),
                output: Box::pin(output),
                stderr,
                stdout: None,
                port: None,
                process: Box::new(process),
            });
        };
        let mut stdout = stdout;
        let mut stderr = stderr;
        let connected = self.connect(spec, port, &mut process, &mut stdout, &mut stderr, cancel);
        let (stream, stdout, stderr) = match connected {
            Ok(connected) => connected,
            Err(err) => {
                // Its group too, even when the adapter itself has exited:
                // what it started may not have. Dropping `process` then
                // reaps it.
                process.kill();
                return Err(err);
            }
        };
        let (output, input) = stream.split();
        Ok(AdapterIo {
            input: Box::pin(input),
            output: Box::pin(output),
            stderr,
            stdout,
            port: Some(port),
            process: Box::new(process),
        })
    }
}

/// How much of a TCP adapter's stdout, and of its stderr, is kept while
/// the launcher waits for it to listen. More is read and dropped, so the
/// adapter never blocks on a full pipe.
pub const STARTUP_OUTPUT_LIMIT: usize = 1024 * 1024;

type Output = Pin<Box<dyn AsyncRead + Send>>;

impl LocalLauncher {
    /// Connects to the host's adapter on `127.0.0.1:port`, with
    /// [`connect_within`]'s retries and limit, as it may still be starting.
    fn connect_to(
        &self,
        spec: &AdapterSpec,
        port: u16,
        cancel: &LaunchCancel,
    ) -> io::Result<AdapterIo> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = async_io::block_on(connect_within(
            &*self.clock,
            self.connect_timeout,
            cancel,
            spec,
            port,
            || false,
            || Async::<TcpStream>::connect(address),
        ))?;
        let closed = Arc::new(Flag::default());
        let process = Connection {
            stream: stream.get_ref().try_clone().ok(),
            closed: closed.clone(),
        };
        let (output, input) = stream.split();
        Ok(AdapterIo {
            input: Box::pin(input),
            output: Box::pin(Closing {
                inner: output,
                closed,
            }),
            stderr: None,
            stdout: None,
            port: Some(port),
            process: Box::new(process),
        })
    }

    /// Connects to `127.0.0.1:port` (see [`connect_within`]) while draining
    /// the adapter's stdout and stderr, which come back with what was read
    /// of them first.
    fn connect(
        &self,
        spec: &AdapterSpec,
        port: u16,
        process: &mut LocalProcess,
        stdout: &mut Option<async_process::ChildStdout>,
        stderr: &mut Option<Output>,
        cancel: &LaunchCancel,
    ) -> io::Result<(Async<TcpStream>, Option<Output>, Option<Output>)> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let (stream, stdout_kept, stderr_kept) = async_io::block_on(async {
            let connect = connect_within(
                &*self.clock,
                self.connect_timeout,
                cancel,
                spec,
                port,
                || match process.child.try_status() {
                    Ok(Some(status)) => {
                        process.exit = Some(exit_info(status));
                        true
                    }
                    _ => false,
                },
                || Async::<TcpStream>::connect(address),
            );
            let (connected, out, err) =
                while_draining(connect, stdout.as_mut(), stderr.as_mut(), &spec.name).await;
            connected.map(|stream| (stream, out, err))
        })?;
        let prefixed = |kept: Vec<u8>, rest: Option<Output>| -> Option<Output> {
            rest.map(|rest| Box::pin(futures::io::Cursor::new(kept).chain(rest)) as Output)
        };
        let stdout = stdout.take().map(|s| Box::pin(s) as Output);
        Ok((
            stream,
            prefixed(stdout_kept, stdout),
            prefixed(stderr_kept, stderr.take()),
        ))
    }
}

/// Runs `work` while draining `stdout` and `stderr` (see [`drain`]), and
/// returns its output with what was kept of theirs.
pub(crate) async fn while_draining<T, O, E>(
    work: impl Future<Output = T>,
    stdout: Option<&mut O>,
    stderr: Option<&mut E>,
    name: &str,
) -> (T, Vec<u8>, Vec<u8>)
where
    O: AsyncRead + Unpin,
    E: AsyncRead + Unpin,
{
    let mut stdout_kept = Vec::new();
    let mut stderr_kept = Vec::new();
    let done = {
        let draining = futures::future::join(
            drain(stdout, &mut stdout_kept, name),
            drain(stderr, &mut stderr_kept, name),
        );
        match futures::future::select(pin!(work), pin!(draining)).await {
            Either::Left((done, _)) => done,
            Either::Right(_) => unreachable!("draining never ends"),
        }
    };
    (done, stdout_kept, stderr_kept)
}

/// Pending once, having woken itself: what a loop whose reads may always
/// be ready awaits between them, so the futures polled beside it run too.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

/// Reads `pipe` into `kept`, up to [`STARTUP_OUTPUT_LIMIT`] (the rest is
/// dropped, and logged once), until it ends; then never resolves. Each read
/// lands in scratch space first, so dropping this loses nothing read, and
/// each is followed by a yield, so a pipe that is always readable never
/// keeps the other stream, the connection, its deadline or a cancel from
/// being polled.
async fn drain<P: AsyncRead + Unpin>(pipe: Option<&mut P>, kept: &mut Vec<u8>, name: &str) {
    if let Some(pipe) = pipe {
        let mut scratch = vec![0; 8 * 1024];
        let mut dropped = false;
        while let Ok(read @ 1..) = pipe.read(&mut scratch).await {
            let keep = read.min(STARTUP_OUTPUT_LIMIT.saturating_sub(kept.len()));
            kept.extend_from_slice(&scratch[..keep]);
            if keep < read && !dropped {
                dropped = true;
                log::warn!(
                    "debug adapter {name} wrote over {STARTUP_OUTPUT_LIMIT} bytes before it \
                     listened; the rest of its startup output is dropped"
                );
            }
            YieldNow(false).await;
        }
    }
    futures::future::pending::<()>().await
}

/// [`connect_within`] with [`CONNECT_TIMEOUT`].
#[cfg(test)]
pub(crate) async fn connect_with<T, F>(
    clock: &dyn Clock,
    cancel: &LaunchCancel,
    spec: &AdapterSpec,
    port: u16,
    exited: impl FnMut() -> bool,
    attempt: impl FnMut() -> F,
) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    connect_within(clock, CONNECT_TIMEOUT, cancel, spec, port, exited, attempt).await
}

/// The connection loop: `attempt` until one succeeds, retrying every
/// [`CONNECT_RETRY`]. It fails as `TimedOut` once `timeout` (by default
/// [`CONNECT_TIMEOUT`], [`LocalLauncher::with_connect_timeout`]) has passed
/// on `clock` since it began, whether an attempt is in flight or not, and
/// starts no attempt and keeps no connection after that; as
/// `ConnectionRefused` once `exited` says the adapter has gone; and as
/// `Interrupted` when `cancel` fires.
pub(crate) async fn connect_within<T, F>(
    clock: &dyn Clock,
    timeout: Duration,
    cancel: &LaunchCancel,
    spec: &AdapterSpec,
    port: u16,
    mut exited: impl FnMut() -> bool,
    mut attempt: impl FnMut() -> F,
) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    let start = clock.now();
    let mut deadline = clock.sleep(timeout);
    let mut fired = pin!(cancel.cancelled());
    let timed_out = || {
        io::Error::new(
            io::ErrorKind::TimedOut,
            tr!(
                "dap-launch-connect-timeout",
                name = spec.name.as_str(),
                port = port,
                seconds = timeout.as_secs()
            )
            .to_string(),
        )
    };
    loop {
        if cancel.is_cancelled() {
            return Err(cancelled(spec));
        }
        let elapsed = clock.now().saturating_duration_since(start);
        if elapsed >= timeout {
            return Err(timed_out());
        }
        // The deadline and a cancel are polled first, and a connection
        // made is kept only if neither has come: one that completes in the
        // same poll as the deadline is too late.
        let stop = futures::future::select(fired.as_mut(), deadline.as_mut());
        match futures::future::select(stop, pin!(attempt())).await {
            Either::Left((Either::Left(_), _)) => return Err(cancelled(spec)),
            Either::Left((Either::Right(_), _)) => return Err(timed_out()),
            Either::Right((Ok(connected), _)) => {
                if cancel.is_cancelled() {
                    return Err(cancelled(spec));
                }
                if clock.now().saturating_duration_since(start) >= timeout {
                    return Err(timed_out());
                }
                return Ok(connected);
            }
            Either::Right((Err(err), _)) => {
                log::debug!("debug adapter {} not listening yet: {err}", spec.name);
            }
        }
        if exited() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                tr!("dap-launch-exited", name = spec.name.as_str()).to_string(),
            ));
        }
        let elapsed = clock.now().saturating_duration_since(start);
        let wait = CONNECT_RETRY.min(timeout.saturating_sub(elapsed));
        futures::future::select(clock.sleep(wait), fired.as_mut()).await;
    }
}

fn cancelled(spec: &AdapterSpec) -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        tr!("dap-launch-cancelled", name = spec.name.as_str()).to_string(),
    )
}

/// A port on `127.0.0.1` that was free a moment ago.
fn free_port() -> io::Result<u16> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

/// What to run for `command` in `cwd`: an absolute command as it is, a
/// relative one with a separator under `cwd`, and a bare name the first
/// runnable file in an absolute directory of `path` (or of this process's
/// `PATH`). `None` for a bare name not found.
fn resolve_program(command: &Path, path: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    if command.is_absolute() {
        return Some(command.to_owned());
    }
    let bare = !command
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&byte| std::path::is_separator(char::from(byte)))
        && !command.has_root();
    if !bare {
        return Some(cwd.join(command));
    }
    let process_path;
    let path = match path {
        Some(path) => path,
        None => {
            process_path = std::env::var_os("PATH")?;
            &process_path
        }
    };
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| candidates(&dir, command))
        .find(|candidate| is_runnable(candidate))
}

#[cfg(unix)]
fn candidates(dir: &Path, name: &Path) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

#[cfg(windows)]
fn candidates(dir: &Path, name: &Path) -> Vec<PathBuf> {
    let mut exe = dir.join(name).into_os_string();
    exe.push(".exe");
    vec![dir.join(name), PathBuf::from(exe)]
}

#[cfg(unix)]
pub(crate) fn is_runnable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(file)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
pub(crate) fn is_runnable(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file())
}

fn exit_info(status: std::process::ExitStatus) -> ExitInfo {
    ExitInfo {
        code: status.code(),
        #[cfg(unix)]
        signal: status.signal(),
        #[cfg(not(unix))]
        signal: None,
    }
}

/// The handle on an [`AdapterTransport::Port`] adapter, which isn't ours:
/// [`kill`](AdapterProcess::kill) only closes the connection, and
/// [`exited`](AdapterProcess::exited) resolves once the connection has
/// closed, by that kill or from the adapter's end ([`Closing`]).
struct Connection {
    stream: Option<TcpStream>,
    closed: Arc<Flag>,
}

/// A Port adapter's output, which marks its connection closed once a read
/// finds its end or fails: the adapter has gone, as far as fe-dap can
/// tell.
struct Closing<R> {
    inner: R,
    closed: Arc<Flag>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Closing<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        let ended = match &polled {
            std::task::Poll::Ready(Ok(0)) => !buf.is_empty(),
            std::task::Poll::Ready(Err(err)) => err.kind() != io::ErrorKind::Interrupted,
            _ => false,
        };
        if ended {
            self.closed.fire();
        }
        polled
    }
}

impl AdapterProcess for Connection {
    fn id(&self) -> Option<u32> {
        None
    }

    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        let closed = self.closed.receiver.clone();
        Box::pin(async move {
            let _ = closed.await;
            ExitInfo::default()
        })
    }

    fn kill(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        self.closed.fire();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.kill();
    }
}

struct LocalProcess {
    child: async_process::Child,
    exit: Option<ExitInfo>,
}

/// An adapter dropped without having been seen to exit (a failed connect,
/// or its session thread panicked) takes its group with it: `kill_on_drop`
/// alone would leave the debuggee running.
impl Drop for LocalProcess {
    fn drop(&mut self) {
        if self.exit.is_none() {
            self.kill();
            let _ = async_io::block_on(self.child.status());
        }
    }
}

impl AdapterProcess for LocalProcess {
    fn id(&self) -> Option<u32> {
        Some(self.child.id())
    }

    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(async move {
            if let Some(exit) = self.exit {
                return exit;
            }
            let exit = match self.child.status().await {
                Ok(status) => exit_info(status),
                Err(err) => {
                    log::warn!("waiting for debug adapter {}: {err}", self.child.id());
                    ExitInfo::default()
                }
            };
            self.exit = Some(exit);
            exit
        })
    }

    fn kill(&mut self) {
        // The adapter leads its group (`process_group(0)`), so the group id
        // is its pid. Signalled even after the leader was reaped, so the
        // children it left behind go too: a pid isn't reused while its group
        // still has members. ESRCH means the group has already gone.
        #[cfg(unix)]
        {
            let group = i32::try_from(self.child.id())
                .ok()
                .and_then(rustix::process::Pid::from_raw);
            if let Some(group) = group
                && let Err(err) =
                    rustix::process::kill_process_group(group, rustix::process::Signal::KILL)
                && err != rustix::io::Errno::SRCH
            {
                log::warn!("killing debug adapter group {}: {err}", self.child.id());
            }
        }
        let _ = self.child.kill();
    }
}
