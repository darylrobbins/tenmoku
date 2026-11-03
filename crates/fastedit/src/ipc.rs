//! Handing a `tenmoku` launch to the running instance, over a Unix socket at
//! `<temp dir>/tenmoku-<uid>/ipc.sock`.
//!
//! One JSON line each way: a [`Request`] (`{targets:[{path,line,col}],
//! new_window, wait, cwd}`) and a reply (`{ok}`). [`start`] decides what a
//! launch is: a client that hands its request over and exits, a detached
//! server's parent, a server, or, when none of those can be, a process on its
//! own, so that no failure here loses a file.
//!
//! The server holds `ipc.lock` for its whole life, so the kernel says whether
//! one is alive: a socket file is stale exactly when the lock is free, however
//! the old server ended and however many launches race to replace it.
//!
//! `TENMOKU_NO_IPC=1` skips all of it, for tests and `cargo run`.
//! `TENMOKU_FOREGROUND=1` stays attached to the terminal, logging included.

use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, TryLockError},
    io::{self, IsTerminal as _, Read as _, Write as _},
    os::unix::{
        ffi::{OsStrExt as _, OsStringExt as _},
        fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
        net::{UnixListener, UnixStream},
        process::CommandExt as _,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use fe_workbench::{DocumentRoute, Workbench, route_document};
use gpui::{App, Task, WindowHandle};
use serde::{Deserialize, Serialize};

use crate::cli::{Cli, Target};

/// Skips both sides of the handoff.
const NO_IPC: &str = "TENMOKU_NO_IPC";
/// Marks the re-executed server, which must not detach again.
const FOREGROUND: &str = "TENMOKU_FOREGROUND";

/// What a Unix socket's path may be, with room for the terminating NUL, on the
/// platform with the shorter limit (macOS's `sun_path`).
const SOCKET_PATH_MAX: usize = 104;
/// How long a launch tries to reach a server that's on its way up, and a
/// client waits for its reply. A server this late is hung, and the launch
/// opens its files itself.
const REPLY_WAIT: Duration = Duration::from_secs(20);
/// How long a server gives a client to send its one line.
const REQUEST_WAIT: Duration = Duration::from_secs(5);
/// How long a detached server waits for its parent's request before it goes
/// by its own arguments, which name the same files.
const FIRST_REQUEST_WAIT: Duration = Duration::from_secs(10);
/// Far beyond what an argument list can hold.
const LINE_MAX: u64 = 16 * 1024 * 1024;
/// Requests waiting for the main thread before the listener stops reading.
const QUEUED_MAX: usize = 64;
/// Between tries at a server that's on its way up.
const RETRY: Duration = Duration::from_millis(20);
/// Between a server's looks at its files, which the system's temp cleaners can
/// delete under a server that runs for days.
const KEEP_EVERY: Duration = Duration::from_secs(30);

/// One launch's request of the running instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) targets: Vec<Target>,
    pub(crate) new_window: bool,
    /// Carried; the server answers at once until `--wait` is acted on.
    pub(crate) wait: bool,
    pub(crate) cwd: PathBuf,
}

/// A path on the wire: a string, or its bytes where it isn't UTF-8 (Linux
/// allows that, and JSON strings can't hold it).
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum WirePath {
    Text(String),
    Bytes(Vec<u8>),
}

impl From<&Path> for WirePath {
    fn from(path: &Path) -> Self {
        match path.to_str() {
            Some(text) => WirePath::Text(text.to_owned()),
            None => WirePath::Bytes(path.as_os_str().as_bytes().to_owned()),
        }
    }
}

impl From<WirePath> for PathBuf {
    fn from(path: WirePath) -> Self {
        match path {
            WirePath::Text(text) => text.into(),
            WirePath::Bytes(bytes) => OsString::from_vec(bytes).into(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct WireTarget {
    path: WirePath,
    line: Option<u32>,
    col: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireRequest {
    targets: Vec<WireTarget>,
    new_window: bool,
    wait: bool,
    cwd: WirePath,
}

#[derive(Debug, Deserialize, Serialize)]
struct Reply {
    ok: bool,
}

impl Request {
    /// What `cli` asks of the running instance, from `cwd`.
    pub(crate) fn of(cli: &Cli, cwd: PathBuf) -> Self {
        Self {
            targets: cli.targets.clone(),
            new_window: cli.new_window,
            wait: cli.wait,
            cwd,
        }
    }

    /// The request as its one line, without the line break: JSON escapes any
    /// in a path.
    pub(crate) fn to_line(&self) -> String {
        let wire = WireRequest {
            targets: (self.targets.iter())
                .map(|target| WireTarget {
                    path: target.path.as_path().into(),
                    line: target.line,
                    col: target.col,
                })
                .collect(),
            new_window: self.new_window,
            wait: self.wait,
            cwd: self.cwd.as_path().into(),
        };
        serde_json::to_string(&wire).expect("the wire types always serialize")
    }

    pub(crate) fn from_line(line: &str) -> serde_json::Result<Self> {
        let wire: WireRequest = serde_json::from_str(line)?;
        Ok(Self {
            targets: (wire.targets.into_iter())
                .map(|target| Target {
                    path: target.path.into(),
                    line: target.line,
                    col: target.col,
                })
                .collect(),
            new_window: wire.new_window,
            wait: wire.wait,
            cwd: wire.cwd.into(),
        })
    }
}

/// This user's socket under `base`, or under `/tmp` if `base` is too long a
/// path to hold one.
pub(crate) fn socket_path(base: &Path) -> PathBuf {
    // SAFETY: getuid has no preconditions and can't fail.
    let uid = unsafe { libc::getuid() };
    let under = |base: &Path| base.join(format!("tenmoku-{uid}")).join("ipc.sock");
    let path = under(base);
    if path.as_os_str().len() < SOCKET_PATH_MAX {
        path
    } else {
        under(Path::new("/tmp"))
    }
}

/// Makes `socket`'s folder this user's alone. One that's someone else's, or a
/// symlink, is refused: `/tmp` is anyone's to create names in, and a server
/// found through such a folder could be anyone's too.
fn secure_dir(socket: &Path) -> io::Result<()> {
    let dir = socket.parent().unwrap_or(Path::new("/"));
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    let meta = fs::symlink_metadata(dir)?;
    // SAFETY: getuid has no preconditions and can't fail.
    if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} isn't this user's folder", dir.display()),
        ));
    }
    if meta.mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// The server's end: this process holds the lock and is bound to the socket.
pub(crate) struct Server {
    socket: PathBuf,
    listener: UnixListener,
    lock: File,
}

/// Takes the lock beside `socket`: `None` if another process holds it.
fn take_lock(socket: &Path) -> io::Result<Option<File>> {
    let lock = (fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true))
    .mode(0o600)
    .open(socket.with_extension("lock"))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Binds `socket`, in place of a file left there.
fn bind(socket: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn inode(path: &Path) -> Option<u64> {
    fs::symlink_metadata(path).ok().map(|meta| meta.ino())
}

impl Server {
    /// Becomes the server for `socket`, if none is alive: `None` if one is, or
    /// is on its way up. A socket file left by a server that has gone is
    /// removed.
    fn claim(socket: &Path) -> io::Result<Option<Server>> {
        let Some(lock) = take_lock(socket)? else {
            return Ok(None);
        };
        Ok(Some(Server {
            socket: socket.to_owned(),
            listener: bind(socket)?,
            lock,
        }))
    }

    /// Listens on a background thread: each request, as it's read, for the
    /// main thread to act on and [answer](Handoff::answer).
    pub(crate) fn listen(self) -> async_channel::Receiver<Handoff> {
        let (mut keeper, handoffs) = self.serve();
        let spawned = std::thread::Builder::new()
            .name("ipc keeper".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(KEEP_EVERY);
                    if !keeper.keep() {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            log::error!(target: "fastedit", "ipc: can't look after the socket: {error}");
        }
        handoffs
    }

    /// [`Server::listen`], with the looking after left to the caller.
    fn serve(self) -> (Keeper, async_channel::Receiver<Handoff>) {
        let (sender, handoffs) = async_channel::bounded(QUEUED_MAX);
        let bound = inode(&self.socket);
        accept_on_a_thread(self.listener, sender.clone());
        let keeper = Keeper {
            socket: self.socket,
            lock: self.lock,
            bound,
            sender,
        };
        (keeper, handoffs)
    }
}

/// Reads requests off `listener` until the main thread stops taking them.
fn accept_on_a_thread(listener: UnixListener, sender: async_channel::Sender<Handoff>) {
    let spawned = std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || {
            // One at a time, each with a time limit: only this user can
            // connect, and clients are gone within moments.
            for stream in listener.incoming() {
                match stream.and_then(Handoff::read) {
                    Ok(handoff) => {
                        if sender.send_blocking(handoff).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        log::warn!(target: "fastedit", "ipc: a request failed: {error}");
                        std::thread::sleep(RETRY);
                    }
                }
            }
        });
    if let Err(error) = spawned {
        log::error!(target: "fastedit", "ipc: can't listen: {error}");
    }
}

/// Holds the server's lock until the process ends, when the kernel lets go of
/// it, and puts back the files a temp cleaner takes: with no socket, launches
/// would find no one to hand over to and no way to serve; with no lock, the
/// next would serve beside this one.
struct Keeper {
    socket: PathBuf,
    lock: File,
    /// The socket file this process bound.
    bound: Option<u64>,
    sender: async_channel::Sender<Handoff>,
}

impl Keeper {
    /// One look at the files. `false` once another process is the server.
    fn keep(&mut self) -> bool {
        let locked = inode(&self.socket.with_extension("lock"));
        let lock_there = locked.is_some() && locked == self.lock.metadata().ok().map(|m| m.ino());
        let socket_there = self.bound.is_some() && inode(&self.socket) == self.bound;
        if lock_there && socket_there {
            return true;
        }
        let rebound = secure_dir(&self.socket).and_then(|()| {
            if !lock_there {
                match take_lock(&self.socket)? {
                    Some(lock) => self.lock = lock,
                    None => return Ok(false),
                }
            }
            let listener = bind(&self.socket)?;
            self.bound = inode(&self.socket);
            // The old thread stays, in an accept nothing can reach.
            accept_on_a_thread(listener, self.sender.clone());
            Ok(true)
        });
        rebound.unwrap_or_else(|error| {
            log::warn!(target: "fastedit", "ipc: can't restore the socket: {error}");
            true
        })
    }
}

/// A request read from a client, and the connection to answer it on.
#[derive(Debug)]
pub(crate) struct Handoff {
    pub(crate) request: Request,
    stream: UnixStream,
}

impl Handoff {
    fn read(mut stream: UnixStream) -> io::Result<Self> {
        let read = read_line(&mut stream, Instant::now() + REQUEST_WAIT).and_then(|line| {
            Request::from_line(&line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        });
        match read {
            Ok(request) => Ok(Self { request, stream }),
            Err(error) => {
                write_line(&mut stream, r#"{"ok":false}"#).ok();
                Err(error)
            }
        }
    }

    /// Tells the client the request is taken.
    pub(crate) fn answer(mut self) {
        // A client that's gone doesn't need telling.
        write_line(&mut self.stream, r#"{"ok":true}"#).ok();
    }
}

/// Reads up to the first line break, until `deadline`. A connection closed
/// before one comes is [`io::ErrorKind::UnexpectedEof`].
fn read_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<String> {
    let mut line = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        allow_closed_peer(stream.set_read_timeout(Some(left)))?;
        let read = match stream.read(&mut chunk) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::ErrorKind::TimedOut.into());
            }
            Err(error) => return Err(error),
        };
        line.extend_from_slice(&chunk[..read]);
        if let Some(end) = line.iter().position(|byte| *byte == b'\n') {
            line.truncate(end);
            return String::from_utf8(line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
        if line.len() as u64 > LINE_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a line too long",
            ));
        }
    }
}

/// The outcome of setting a socket's timeout. macOS refuses to once the peer
/// has closed, which a server does right after answering: reads then finish at
/// once, with what it said, and writes fail, so neither needs a timeout.
fn allow_closed_peer(set: io::Result<()>) -> io::Result<()> {
    match set {
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => Ok(()),
        set => set,
    }
}

fn write_line(stream: &mut UnixStream, line: &str) -> io::Result<()> {
    allow_closed_peer(stream.set_write_timeout(Some(REQUEST_WAIT)))?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")
}

/// How a try at handing a request over went.
enum Handed {
    /// The server took it.
    Taken,
    /// No server answered: no socket, a refused connection, or one that closed
    /// with no reply, the server having just gone.
    NoServer,
    /// A server is there, but didn't take it: it refused, or never replied.
    Failed(io::Error),
}

fn hand_over(socket: &Path, request: &Request) -> Handed {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return Handed::NoServer;
    };
    let reply = write_line(&mut stream, &request.to_line())
        .and_then(|()| read_line(&mut stream, Instant::now() + REPLY_WAIT));
    match reply {
        Ok(line) => match serde_json::from_str(&line) {
            Ok(Reply { ok: true }) => Handed::Taken,
            Ok(Reply { ok: false }) => Handed::Failed(io::Error::other("the server refused")),
            Err(error) => Handed::Failed(error.into()),
        },
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
            ) =>
        {
            Handed::NoServer
        }
        Err(error) => Handed::Failed(error),
    }
}

/// How this process was started, as far as the handoff cares.
#[derive(Clone, Debug)]
pub(crate) struct How {
    /// `TENMOKU_NO_IPC=1`, or a run that must be its own process (`--perf`).
    pub(crate) no_ipc: bool,
    /// `TENMOKU_FOREGROUND=1`: stays attached, whatever else holds.
    pub(crate) foreground: bool,
    /// Any of stdin, stdout or stderr is a terminal.
    pub(crate) terminal: bool,
    /// Started by macOS's LaunchServices (Finder, `open`).
    pub(crate) launch_services: bool,
    /// Where the socket's folder goes.
    pub(crate) temp_dir: PathBuf,
}

impl How {
    /// Whether this is the detached server of a `wait` launch, whose parent
    /// stays and carries the request itself. (By hand, `TENMOKU_FOREGROUND=1`
    /// comes with a terminal, and has no such parent.)
    pub(crate) fn parent_carries_request(&self, wait: bool) -> bool {
        self.foreground && wait && !self.terminal
    }

    /// `how` from the environment variables `var` gives.
    pub(crate) fn from_env(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name| var(name).is_some_and(|value| value == OsStr::new("1"));
        Self {
            no_ipc: set(NO_IPC),
            foreground: set(FOREGROUND),
            terminal: false,
            launch_services: false,
            temp_dir: std::env::temp_dir(),
        }
    }

    /// This process's. Takes [`FOREGROUND`] out of the environment, for it's
    /// meant for this process, not for what the editor's terminals run. Call
    /// it before there are other threads.
    pub(crate) fn current() -> Self {
        let mut how = Self::from_env(|name| std::env::var_os(name));
        how.terminal = std::io::stdin().is_terminal()
            || std::io::stdout().is_terminal()
            || std::io::stderr().is_terminal();
        how.launch_services = cfg!(target_os = "macos")
            && by_launch_services(
                how.terminal,
                how.foreground,
                std::os::unix::process::parent_id(),
            );
        // SAFETY: the caller is the process's only thread so far.
        unsafe { std::env::remove_var(FOREGROUND) };
        how
    }
}

/// Whether a macOS process was started by LaunchServices: launchd is its
/// parent and it has no terminal. A re-executed server looks the same once its
/// parent exits, but is marked `foreground`.
fn by_launch_services(terminal: bool, foreground: bool, parent: u32) -> bool {
    !terminal && !foreground && parent == 1
}

/// What a launch goes on to do.
pub(crate) enum Start {
    /// The launch is over, in the running instance or a detached server.
    Exit,
    /// Run the app and serve what later launches hand over.
    Serve(Server),
    /// Run the app, alone.
    Alone,
}

/// Decides what this launch is: see the module's docs. `detach` starts the
/// detached server ([`detach`], when it isn't a test).
///
/// A launch by LaunchServices with no paths in its arguments is never a
/// client, for what it's to open comes as open-document events only it
/// receives; it serves if nothing else does, and otherwise runs as it always
/// has.
pub(crate) fn start(
    request: &Request,
    how: &How,
    detach: impl FnOnce() -> io::Result<()>,
) -> Start {
    if how.no_ipc {
        return Start::Alone;
    }
    let socket = socket_path(&how.temp_dir);
    if let Err(error) = secure_dir(&socket) {
        log::warn!(target: "fastedit", "ipc: running alone: {error}");
        return Start::Alone;
    }
    let client = !how.launch_services || !request.targets.is_empty();
    let parent_carries = how.parent_carries_request(request.wait);
    let deadline = Instant::now() + REPLY_WAIT;
    if client && !parent_carries {
        match hand_over(&socket, request) {
            Handed::Taken => return Start::Exit,
            Handed::NoServer => {}
            Handed::Failed(error) => {
                log::warn!(target: "fastedit", "ipc: running alone: {error}");
                return Start::Alone;
            }
        }
    }
    // A LaunchServices launch can't be re-executed: its events would be lost.
    let detaches = !how.foreground && !how.launch_services && (how.terminal || request.wait);
    if detaches {
        match detach() {
            Ok(()) if !request.wait => return Start::Exit,
            // The parent stays as the client of the server it started.
            Ok(()) => return wait_for_server(&socket, request, deadline),
            Err(error) => log::warn!(target: "fastedit", "ipc: staying attached: {error}"),
        }
    }
    loop {
        match Server::claim(&socket) {
            Ok(Some(server)) => return Start::Serve(server),
            // Someone else serves, and the parent will find them.
            Ok(None) if parent_carries => return Start::Exit,
            Ok(None) if !client => return Start::Alone,
            Ok(None) => {}
            Err(error) => {
                log::warn!(target: "fastedit", "ipc: running alone: {error}");
                return Start::Alone;
            }
        }
        // Another launch is on its way to serving, unless it dies first, when
        // the lock is free again.
        if let Some(start) = try_server(&socket, request, deadline) {
            return start;
        }
    }
}

/// Hands `request` to a server on its way up, trying until `deadline`: alone
/// if none answers by then, or if one fails to take it.
fn wait_for_server(socket: &Path, request: &Request, deadline: Instant) -> Start {
    loop {
        if let Some(start) = try_server(socket, request, deadline) {
            return start;
        }
    }
}

/// One try at handing `request` to a server on its way up: `None` to try again
/// after a moment.
fn try_server(socket: &Path, request: &Request, deadline: Instant) -> Option<Start> {
    match hand_over(socket, request) {
        Handed::Taken => Some(Start::Exit),
        Handed::NoServer if Instant::now() < deadline => {
            std::thread::sleep(RETRY);
            None
        }
        Handed::NoServer => Some(Start::Alone),
        Handed::Failed(error) => {
            log::warn!(target: "fastedit", "ipc: running alone: {error}");
            Some(Start::Alone)
        }
    }
}

/// Starts the server this launch detaches from: itself again, with the same
/// arguments, in its own process group, with no terminal.
pub(crate) fn detach() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    detached_command(&exe, std::env::args_os().skip(1))
        .spawn()
        .map(drop)
}

fn detached_command(exe: &Path, args: impl Iterator<Item = OsString>) -> Command {
    let mut command = Command::new(exe);
    command
        .args(args)
        .env(FOREGROUND, "1")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// What a detached server launches when its parent stayed as the client
/// (`--wait`): the first request, whose connection is the client's to wait on,
/// instead of its own arguments; a window opened from those as well would show
/// the request's files twice. A client that doesn't come leaves `cli` as is.
pub(crate) fn launch_first_request(handoffs: &async_channel::Receiver<Handoff>, cli: &mut Cli) {
    let deadline = Instant::now() + FIRST_REQUEST_WAIT;
    while Instant::now() < deadline {
        if let Ok(handoff) = handoffs.try_recv() {
            cli.targets.clone_from(&handoff.request.targets);
            cli.new_window = handoff.request.new_window;
            handoff.answer();
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Acts on `request`, one target at a time so the tabs keep their order
/// whichever read is quickest. With `new_window`, a window opens on the first
/// folder or the first file's folder, as a launch would, with the files in it.
/// Otherwise each goes by [`route_document`]; there, `new_window` is what opens
/// a window on a folder. The app is activated.
pub(crate) fn route(
    request: Request,
    cx: &mut App,
    new_window: impl Fn(Option<PathBuf>, &mut App) -> Option<WindowHandle<Workbench>> + 'static,
) -> Task<()> {
    let targets = (request.targets.into_iter())
        .map(|target| Target {
            // Absolute already, as the command line makes them.
            path: request.cwd.join(&target.path),
            ..target
        })
        .collect();
    let cli = Cli {
        perf: false,
        new_window: request.new_window,
        wait: request.wait,
        targets,
    };
    let (window, targets) = if request.new_window {
        let plan = cli.plan();
        (new_window(plan.root, cx), plan.files)
    } else {
        (None, cli.targets)
    };
    if let Some(window) = window {
        window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    }
    cx.activate(true);
    cx.spawn(async move |cx| {
        for target in targets {
            let landed = cx.update(|cx| open_target(target, window, &new_window, cx));
            if let Some(landed) = landed {
                landed.await;
            }
        }
    })
}

/// Opens `target` in `window`, or where [`route_document`] says with none, and
/// brings that window up: what's left of it is the task's.
fn open_target(
    target: Target,
    window: Option<WindowHandle<Workbench>>,
    new_window: &impl Fn(Option<PathBuf>, &mut App) -> Option<WindowHandle<Workbench>>,
    cx: &mut App,
) -> Option<Task<()>> {
    let Target { path, line, col } = target;
    let (window, file) = match window {
        Some(window) => (window, Some(path)),
        None => {
            let windows: Vec<_> = (cx.windows().into_iter())
                .filter_map(|window| window.downcast::<Workbench>())
                .collect();
            let roots: Vec<_> = (windows.iter())
                .map(|window| {
                    window
                        .read_with(cx, |workbench, _| workbench.root().map(Path::to_owned))
                        .ok()
                        .flatten()
                })
                .collect();
            match route_document(&path, &roots) {
                DocumentRoute::Window { window, file } => (windows[window], file),
                DocumentRoute::NewWindow { root, file } => (new_window(Some(root), cx)?, file),
            }
        }
    };
    window
        .update(cx, |workbench, window, cx| {
            window.activate_window();
            let file = file?;
            Some(workbench.open_document_at(file, line, col, window, cx))
        })
        .ok()
        .flatten()
}

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
