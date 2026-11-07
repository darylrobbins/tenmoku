//! Handing a `tenmoku` launch to the running instance, over a Unix socket at
//! `<temp dir>/tenmoku-<uid>/ipc.sock`.
//!
//! One JSON line each way: a [`Request`] (`{targets:[{path,line,col}],
//! new_window, wait, cwd}`) and a reply (`{ok}`). A request with `wait` then
//! has the server hold the connection, and a second line, `{closed}`, comes
//! once its files have no tab left ([`crate::wait`]): `{ok}` still comes at
//! once, for a client can tell a hung server only by its silence, and goes on
//! by itself then. [`start`] decides what a launch is: a client that hands its
//! request over and exits, a detached server's parent, a server, or, when none
//! of those can be, a process on its own, so that no failure here loses a file.
//!
//! A waiting client is a plain blocking read with the system's handling of
//! signals: SIGINT or SIGTERM ends it as they end any command, the kernel
//! closes its connection, and the server, which reads that end for just this
//! ([`Waiter::watch`]), forgets the wait. Nothing else holds the connection:
//! the detached server started before it was made, and it's close-on-exec.
//!
//! The server holds `ipc.lock` for its whole life, so the kernel says whether
//! one is alive: a socket file is stale exactly when the lock is free, however
//! the old server ended and however many launches race to replace it.
//!
//! `TENMOKU_NO_IPC=1` skips all of it, for tests and `cargo run`.
//! `TENMOKU_FOREGROUND=1` stays attached to the terminal, logging included.
//! [`PARENT_CARRIES`] is the detached server's own, never for setting by hand.

//! Windows has no Unix sockets in std, so there every launch runs alone
//! ([`start`]); the rest builds against [`unix_socket`]'s stand-ins, which
//! nothing can make.

// Where the stand-ins hold a connection, what follows can't be reached.
#![cfg_attr(windows, allow(unreachable_code, unused_variables))]

#[cfg(windows)]
use self::unix_socket::{UnixListener, UnixStream};
#[cfg(unix)]
use std::os::unix::{
    ffi::OsStringExt as _,
    fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _},
    net::{UnixListener, UnixStream},
    process::CommandExt as _,
};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, TryLockError},
    io::{self, IsTerminal as _, Read as _, Write as _},
    net::Shutdown,
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
/// Marks a re-executed server whose parent stayed behind as the client of a
/// `--wait` launch, and carries its request.
const PARENT_CARRIES: &str = "TENMOKU_PARENT_CARRIES";

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
/// delete under a server that runs for days. Well inside [`REPLY_WAIT`]: a
/// launch that finds the socket gone keeps trying that long, so the server that
/// still holds the lock has put it back before the launch gives up on it.
const KEEP_EVERY: Duration = Duration::from_secs(5);
const _: () = assert!(KEEP_EVERY.as_secs() * 2 < REPLY_WAIT.as_secs());

/// One launch's request of the running instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) targets: Vec<Target>,
    pub(crate) new_window: bool,
    /// Hold the connection until the files have no tab left.
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
            None => WirePath::Bytes(path.as_os_str().as_encoded_bytes().to_owned()),
        }
    }
}

impl From<WirePath> for PathBuf {
    fn from(path: WirePath) -> Self {
        match path {
            WirePath::Text(text) => text.into(),
            #[cfg(unix)]
            WirePath::Bytes(bytes) => OsString::from_vec(bytes).into(),
            // Bytes from the wire may not be a valid encoding of a name.
            #[cfg(windows)]
            WirePath::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned().into(),
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

/// What a request with `wait` is finally told.
#[derive(Debug, Deserialize)]
struct Closed {
    closed: bool,
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
    let uid = user_id();
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
#[cfg(unix)]
fn user_id() -> libc::uid_t {
    // SAFETY: getuid has no preconditions and can't fail.
    unsafe { libc::getuid() }
}

/// The user's name, for the socket's folder: there is no socket on Windows.
#[cfg(windows)]
fn user_id() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// No folder on Windows, where the handoff never runs.
#[cfg(windows)]
fn secure_dir(_: &Path) -> io::Result<()> {
    Err(unix_socket::unsupported())
}

#[cfg(unix)]
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
    let lock = kiln::core::platform::private(
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true),
    )
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
    #[cfg(unix)]
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn inode(path: &Path) -> Option<u64> {
    let (_, inode) = kiln::core::platform::path_identity(path, false).ok()?;
    Some(inode)
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
        let held = kiln::core::platform::file_identity(&self.lock).ok();
        let lock_there = locked.is_some() && locked == held.map(|(_, inode)| inode);
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

    /// Tells the client the request is taken, and that's all it'll hear: with
    /// `wait`, that there's nothing to wait for.
    pub(crate) fn answer(self) {
        let wait = self.request.wait;
        let waiter = self.hold();
        if wait {
            waiter.closed();
        }
    }

    /// Tells the client the request is taken, keeping the connection to say
    /// when the wait is over.
    pub(crate) fn hold(mut self) -> Waiter {
        // A client that's gone doesn't need telling.
        write_line(&mut self.stream, r#"{"ok":true}"#).ok();
        Waiter {
            stream: self.stream,
        }
    }
}

/// The connection of a client waiting for its files to close. Dropping it
/// forgets the client, who'd take that for a server gone: it's for a client
/// that has gone itself.
#[derive(Debug)]
pub(crate) struct Waiter {
    stream: UnixStream,
}

impl Waiter {
    /// Tells the client the wait is over.
    pub(crate) fn closed(mut self) {
        write_line(&mut self.stream, r#"{"closed":true}"#).ok();
        // Ends the read of [`Waiter::watch`], which holds a copy of the stream.
        self.stream.shutdown(Shutdown::Both).ok();
    }

    /// Runs `gone` on a thread of its own once the connection has closed: when
    /// the client goes, however it does, or after [`Waiter::closed`].
    pub(crate) fn watch(&self, gone: impl FnOnce() + Send + 'static) {
        let spawned = self.stream.try_clone().and_then(|mut stream| {
            std::thread::Builder::new()
                .name("ipc wait".into())
                .spawn(move || {
                    // What reading the request set, which all copies share.
                    allow_closed_peer(stream.set_read_timeout(None)).ok();
                    let mut chunk = [0; 64];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(_) => {}
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                    gone();
                })
        });
        if let Err(error) = spawned {
            log::warn!(target: "fastedit", "ipc: can't watch a waiting client: {error}");
        }
    }

    /// A waiter, and the client's end of its connection.
    #[cfg(all(test, unix))]
    pub(crate) fn pair() -> (Self, UnixStream) {
        let (stream, client) = UnixStream::pair().unwrap();
        (Self { stream }, client)
    }
}

/// Reads up to the first line break, until `deadline`. A connection closed
/// before one comes is [`io::ErrorKind::UnexpectedEof`].
fn read_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<String> {
    next_line(stream, &mut Vec::new(), Some(deadline))
}

/// [`read_line`], for a stream of several lines, or with no time limit at all.
/// `pending` is what the last read took past its line: two lines sent at once
/// arrive at once.
fn next_line(
    stream: &mut UnixStream,
    pending: &mut Vec<u8>,
    deadline: Option<Instant>,
) -> io::Result<String> {
    let mut chunk = [0; 4096];
    loop {
        if let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let rest = pending.split_off(end + 1);
            let mut line = std::mem::replace(pending, rest);
            line.truncate(end);
            return String::from_utf8(line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
        if pending.len() as u64 > LINE_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a line too long",
            ));
        }
        let left = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        if left.is_some_and(|left| left.is_zero()) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        allow_closed_peer(stream.set_read_timeout(left))?;
        let read = match stream.read(&mut chunk) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::ErrorKind::TimedOut.into());
            }
            Err(error) => return Err(error),
        };
        pending.extend_from_slice(&chunk[..read]);
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
    /// The server took it, and with `wait`, the files have closed.
    Taken,
    /// The server took it and went before the files closed: it crashed.
    Lost,
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
    let mut pending = Vec::new();
    let deadline = Instant::now() + REPLY_WAIT;
    let reply = write_line(&mut stream, &request.to_line())
        .and_then(|()| next_line(&mut stream, &mut pending, Some(deadline)));
    match reply {
        Ok(line) => match serde_json::from_str(&line) {
            Ok(Reply { ok: true }) if request.wait => wait_for_close(&mut stream, &mut pending),
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

/// Blocks until the server says the request's files have closed, for as long
/// as that takes.
fn wait_for_close(stream: &mut UnixStream, pending: &mut Vec<u8>) -> Handed {
    let reply = next_line(stream, pending, None);
    match reply.map(|line| serde_json::from_str(&line)) {
        Ok(Ok(Closed { closed: true })) => Handed::Taken,
        Ok(Ok(Closed { closed: false })) => {
            log::error!(target: "fastedit", "ipc: the editor refused to say the files closed");
            Handed::Lost
        }
        Ok(Err(error)) => {
            log::error!(target: "fastedit", "ipc: a bad reply from the editor: {error}");
            Handed::Lost
        }
        Err(error) => {
            log::error!(target: "fastedit", "ipc: the editor went before the files closed: {error}");
            Handed::Lost
        }
    }
}

/// How this process was started, as far as the handoff cares.
#[derive(Clone, Debug)]
pub(crate) struct How {
    /// `TENMOKU_NO_IPC=1`, or a run that must be its own process (`--perf`).
    pub(crate) no_ipc: bool,
    /// `TENMOKU_FOREGROUND=1`: stays attached, whatever else holds.
    pub(crate) foreground: bool,
    /// [`PARENT_CARRIES`]: this is the detached server of a `--wait` launch,
    /// whose parent stays and carries the request itself.
    pub(crate) parent_carries: bool,
    /// Any of stdin, stdout or stderr is a terminal.
    pub(crate) terminal: bool,
    /// Started by macOS's LaunchServices (Finder, `open`).
    pub(crate) launch_services: bool,
    /// Where the socket's folder goes.
    pub(crate) temp_dir: PathBuf,
}

impl How {
    /// Whether a launch with `wait` that goes on to run the app has to end its
    /// own wait, having no other process to do it. Not the detached server,
    /// whose parent waits, nor a launch by LaunchServices: nothing's attached to
    /// it, and the files it's to open may not have come yet.
    pub(crate) fn waits_by_itself(&self, wait: bool) -> bool {
        wait && !self.parent_carries && !self.launch_services
    }

    /// `how` from the environment variables `var` gives.
    pub(crate) fn from_env(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name| var(name).is_some_and(|value| value == OsStr::new("1"));
        Self {
            no_ipc: set(NO_IPC),
            foreground: set(FOREGROUND),
            parent_carries: set(PARENT_CARRIES),
            terminal: false,
            launch_services: false,
            temp_dir: std::env::temp_dir(),
        }
    }

    /// This process's. Takes [`FOREGROUND`] and [`PARENT_CARRIES`] out of the
    /// environment, for they're meant for this process, not for what the
    /// editor's terminals run. Call it before there are other threads.
    pub(crate) fn current() -> Self {
        let mut how = Self::from_env(|name| std::env::var_os(name));
        how.terminal = std::io::stdin().is_terminal()
            || std::io::stdout().is_terminal()
            || std::io::stderr().is_terminal();
        #[cfg(unix)]
        {
            how.launch_services = cfg!(target_os = "macos")
                && by_launch_services(
                    how.terminal,
                    how.foreground,
                    std::os::unix::process::parent_id(),
                );
        }
        // SAFETY: the caller is the process's only thread so far.
        unsafe {
            std::env::remove_var(FOREGROUND);
            std::env::remove_var(PARENT_CARRIES);
        }
        how
    }
}

/// Whether a macOS process was started by LaunchServices: launchd is its
/// parent and it has no terminal. A re-executed server looks the same once its
/// parent exits, but is marked `foreground`.
#[cfg_attr(not(unix), allow(dead_code))]
fn by_launch_services(terminal: bool, foreground: bool, parent: u32) -> bool {
    !terminal && !foreground && parent == 1
}

/// What a launch goes on to do.
pub(crate) enum Start {
    /// The launch is over, in the running instance or a detached server: with
    /// `--wait`, the files have closed too.
    Exit,
    /// A `--wait` launch's server went before the files closed: it crashed.
    /// The launch fails.
    ServerLost,
    /// Run the app and serve what later launches hand over.
    Serve(Server),
    /// Run the app, alone.
    Alone,
}

/// Decides what this launch is: see the module's docs. `detach` starts the
/// detached server ([`detach`], when it isn't a test).
///
/// A launch by LaunchServices is never a client, whatever its arguments, for
/// what it's to open may come as open-document events only it receives, and
/// only once the app is running (`open -na Tenmoku a.txt --args b.txt` sends
/// both); it serves if nothing else does, and otherwise runs as it always has.
pub(crate) fn start(
    request: &Request,
    how: &How,
    detach: impl FnOnce() -> io::Result<()>,
) -> Start {
    // No Unix sockets on Windows: each launch is its own instance.
    if how.no_ipc || cfg!(windows) {
        return Start::Alone;
    }
    let socket = socket_path(&how.temp_dir);
    if let Err(error) = secure_dir(&socket) {
        log::warn!(target: "fastedit", "ipc: running alone: {error}");
        return Start::Alone;
    }
    let client = !how.launch_services;
    let parent_carries = how.parent_carries;
    let deadline = Instant::now() + REPLY_WAIT;
    if client && !parent_carries {
        match hand_over(&socket, request) {
            Handed::Taken => return Start::Exit,
            Handed::Lost => return Start::ServerLost,
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
        Handed::Lost => Some(Start::ServerLost),
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
/// arguments, in its own process group, with no terminal. With `parent_carries`,
/// this process stays as the client of it, and has the request.
pub(crate) fn detach(parent_carries: bool) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    detached_command(&exe, std::env::args_os().skip(1), parent_carries)
        .spawn()
        .map(drop)
}

fn detached_command(
    exe: &Path,
    args: impl Iterator<Item = OsString>,
    parent_carries: bool,
) -> Command {
    let mut command = Command::new(exe);
    if parent_carries {
        command.env(PARENT_CARRIES, "1");
    }
    command
        .args(args)
        .env(FOREGROUND, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    command
}

/// Stand-ins for std's Unix sockets on Windows, where the handoff never runs:
/// neither type has a value, so making one fails and nothing else is reached.
#[cfg(windows)]
mod unix_socket {
    use std::{io, net::Shutdown, path::Path, time::Duration};

    pub(super) fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "no launch handoff on Windows: each launch is its own instance",
        )
    }

    #[derive(Debug)]
    pub(crate) enum UnixStream {}

    impl UnixStream {
        pub(super) fn connect(_: &Path) -> io::Result<UnixStream> {
            Err(unsupported())
        }

        pub(super) fn shutdown(&self, _: Shutdown) -> io::Result<()> {
            match *self {}
        }

        pub(super) fn try_clone(&self) -> io::Result<UnixStream> {
            match *self {}
        }

        pub(super) fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
            match *self {}
        }

        pub(super) fn set_write_timeout(&self, _: Option<Duration>) -> io::Result<()> {
            match *self {}
        }
    }

    impl io::Read for UnixStream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            match *self {}
        }
    }

    impl io::Write for UnixStream {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            match *self {}
        }

        fn flush(&mut self) -> io::Result<()> {
            match *self {}
        }
    }

    #[derive(Debug)]
    pub(crate) enum UnixListener {}

    impl UnixListener {
        pub(super) fn bind(_: &Path) -> io::Result<UnixListener> {
            Err(unsupported())
        }

        pub(super) fn incoming(&self) -> std::iter::Empty<io::Result<UnixStream>> {
            match *self {}
        }
    }
}

/// What a detached server launches when its parent stayed as the client
/// (`--wait`): the first request, whose connection is the client's to wait on,
/// instead of its own arguments; a window opened from those as well would show
/// the request's files twice. A client that doesn't come leaves `cli` as is.
/// Gives who's waiting on them, if that's what the request asks; another launch
/// may have got in first, and the parent's request then comes like any other.
pub(crate) fn launch_first_request(
    handoffs: &async_channel::Receiver<Handoff>,
    cli: &mut Cli,
) -> Option<Waiter> {
    let deadline = Instant::now() + FIRST_REQUEST_WAIT;
    while Instant::now() < deadline {
        if let Ok(handoff) = handoffs.try_recv() {
            cli.targets.clone_from(&handoff.request.targets);
            cli.new_window = handoff.request.new_window;
            if handoff.request.wait {
                return Some(handoff.hold());
            }
            handoff.answer();
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
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
    let landing = window
        .update(cx, |workbench, window, cx| {
            window.activate_window();
            let file = file?;
            Some(workbench.open_document_at(file, line, col, window, cx))
        })
        .ok()
        .flatten()?;
    Some(while_open(window, landing, cx))
}

/// `task`, given up on if `window` closes first: what's opening in a window
/// waits on its panes, and hears nothing more from them once it has gone.
pub(crate) fn while_open(
    window: WindowHandle<Workbench>,
    task: Task<()>,
    cx: &mut App,
) -> Task<()> {
    let id = window.window_id();
    let (done, over) = async_channel::bounded::<()>(2);
    let closed = done.clone();
    let closing = cx.on_window_closed(move |_, gone| {
        if gone == id {
            closed.try_send(()).ok();
        }
    });
    let finishing = cx.spawn(async move |_| {
        task.await;
        done.try_send(()).ok();
    });
    cx.spawn(async move |_| {
        over.recv().await.ok();
        drop((closing, finishing));
    })
}

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
