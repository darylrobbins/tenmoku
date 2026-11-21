//! `tenmoku-server daemon` (D § 3): holds the sessions of one host, user and
//! build, on the socket [`Paths`] names. It exits [`Config::linger`] after
//! its last session ends.

use crate::server::{Shared, Snapshot};
use crate::{Config, Paths, paths};
use fe_remote_proto::{HandshakeError, Link, SessionToken};
use std::fs::{self, File};
use std::io::{self, Seek as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How long a connection has to send its `Hello`. Only this user can
/// connect, and a client's attach sends it at once.
const HELLO_WAIT: Duration = Duration::from_secs(30);
/// Between the keeper's looks at the files, which a temp cleaner can delete
/// under a daemon that runs for days (`/tmp`, the fallback).
const KEEP_EVERY: Duration = Duration::from_secs(5);
/// After a failed accept (out of descriptors, say), before the next.
const ACCEPT_RETRY: Duration = Duration::from_millis(50);

/// The daemon: this process holds the lock and is bound to the socket. A
/// clone is another handle on the same daemon.
#[derive(Clone)]
pub struct Daemon(Arc<Inner>);

struct Inner {
    shared: Arc<Shared>,
    paths: Paths,
    files: Mutex<Files>,
}

struct Files {
    /// Held from [`Daemon::claim`] until [`Daemon::run`] returns.
    lock: Option<File>,
    /// The socket file this process bound.
    bound: Option<u64>,
    /// Until [`Daemon::run`] takes it.
    listener: Option<UnixListener>,
}

impl Daemon {
    /// Becomes `paths`' daemon, if none is alive: `None` if one is, or is on
    /// its way up. A socket file left by a daemon that has gone is replaced.
    pub fn claim(paths: Paths, config: Config) -> io::Result<Option<Daemon>> {
        paths.secure()?;
        let Some(mut lock) = paths.take_lock()? else {
            return Ok(None);
        };
        lock.set_len(0)?;
        lock.rewind()?;
        writeln!(lock, "{}", std::process::id())?;
        let listener = bind(&paths.socket)?;
        let bound = paths::inode(&paths.socket);
        log!(
            "daemon for build {} on {}",
            config.mux.build,
            paths.socket.display()
        );
        Ok(Some(Daemon(Arc::new(Inner {
            shared: Shared::new(config),
            paths,
            files: Mutex::new(Files {
                lock: Some(lock),
                bound,
                listener: Some(listener),
            }),
        }))))
    }

    pub fn paths(&self) -> &Paths {
        &self.0.paths
    }

    /// Serves connections until the daemon has had no session for
    /// [`Config::linger`], then removes its socket and lets go of its lock.
    /// Each connection gets a thread of its own, and is a session's link.
    pub fn run(&self) {
        let Some(listener) = self.files().listener.take() else {
            return;
        };
        self.accept_on_a_thread(listener);
        let keeper = self.clone();
        let kept = thread::Builder::new()
            .name("daemon-keeper".into())
            .spawn(move || keeper.keep());
        if let Err(e) = kept {
            log!("can't look after the socket: {e}");
        }
        self.0.shared.reap(Some(self.0.shared.config.linger));
        log!("no session for {:?}: exiting", self.0.shared.config.linger);
        // Wakes the accept loop, which sees the daemon is exiting.
        drop(UnixStream::connect(&self.0.paths.socket));
        let mut files = self.files();
        if files.bound.is_some() && paths::inode(&self.0.paths.socket) == files.bound {
            let _ = fs::remove_file(&self.0.paths.socket);
        }
        files.bound = None;
        // After the socket: an attach that finds no socket and a held lock
        // waits for the daemon on its way up, and this one is on its way
        // out.
        files.lock = None;
        drop(files);
        self.0.shared.set_exited();
    }

    /// Serves `link` as one of the daemon's sessions, as a connection to
    /// its socket would be, until the link is lost.
    pub fn serve(&self, link: Link) -> Result<SessionToken, HandshakeError> {
        self.0.shared.serve_link(link, || {})
    }

    /// What the daemon holds now.
    pub fn snapshot(&self) -> Snapshot {
        self.0.shared.snapshot()
    }

    /// Waits until `seen` holds of [`Daemon::snapshot`], for up to a minute
    /// of real time: false if it never did.
    pub fn wait_for(&self, seen: impl FnMut(&Snapshot) -> bool) -> bool {
        self.0.shared.wait_for(seen)
    }

    /// Waits for the daemon to have acted on the clock's time now: with a
    /// manual clock, whatever is due by then has happened, and nothing else
    /// will until the clock moves.
    pub fn settle(&self) -> bool {
        self.0.shared.settle()
    }

    /// Spawns `command` as the leader of a new process group that belongs to
    /// `session` and is torn down with it. Gives the leader's pid.
    pub fn spawn_group(&self, session: SessionToken, command: &mut Command) -> io::Result<u32> {
        self.0.shared.spawn_group(session, command)
    }

    fn files(&self) -> std::sync::MutexGuard<'_, Files> {
        self.0.files.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn accept_on_a_thread(&self, listener: UnixListener) {
        let daemon = self.clone();
        let spawned = thread::Builder::new()
            .name("daemon-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if daemon.0.shared.exiting() {
                        return;
                    }
                    match stream {
                        Ok(stream) => daemon.serve_on_a_thread(stream),
                        Err(e) => {
                            log!("an accept failed: {e}");
                            thread::sleep(ACCEPT_RETRY);
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            log!("can't listen: {e}");
        }
    }

    fn serve_on_a_thread(&self, stream: UnixStream) {
        let shared = self.0.shared.clone();
        let spawned = thread::Builder::new()
            .name("daemon-session".into())
            .spawn(move || {
                if let Err(e) = serve(&shared, stream) {
                    log!("a connection failed: {e}");
                }
            });
        if let Err(e) = spawned {
            log!("can't serve a connection: {e}");
        }
    }

    /// Puts back the socket and lock a temp cleaner took, every
    /// [`KEEP_EVERY`], until the daemon exits or another process has become
    /// the daemon: with no socket, attaches would find no one; with no lock,
    /// the next would start a daemon beside this one.
    fn keep(&self) {
        loop {
            thread::sleep(KEEP_EVERY);
            let mut files = self.files();
            if files.lock.is_none() || self.0.shared.exiting() {
                return;
            }
            match self.restore(&mut files) {
                Ok(true) => {}
                Ok(false) => {
                    log!("another daemon holds the lock now: no longer keeping the socket");
                    return;
                }
                Err(e) => log!("can't restore the socket: {e}"),
            }
        }
    }

    /// One look at the files. False once another process holds the lock.
    fn restore(&self, files: &mut Files) -> io::Result<bool> {
        let paths = &self.0.paths;
        let held = files.lock.as_ref().and_then(|lock| {
            use std::os::unix::fs::MetadataExt as _;
            lock.metadata().ok().map(|m| m.ino())
        });
        let lock_there = held.is_some() && paths::inode(&paths.lock) == held;
        let socket_there = files.bound.is_some() && paths::inode(&paths.socket) == files.bound;
        if lock_there && socket_there {
            return Ok(true);
        }
        paths.secure()?;
        if !lock_there {
            let Some(mut lock) = paths.take_lock()? else {
                return Ok(false);
            };
            writeln!(lock, "{}", std::process::id())?;
            files.lock = Some(lock);
        }
        let listener = bind(&paths.socket)?;
        files.bound = paths::inode(&paths.socket);
        log!("put back {}", paths.socket.display());
        // The old accept thread stays, in an accept nothing can reach.
        self.accept_on_a_thread(listener);
        Ok(true)
    }
}

/// Serves one connection: its handshake, then its session until the link
/// is lost.
fn serve(shared: &Shared, stream: UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(HELLO_WAIT))?;
    let timed = stream.try_clone()?;
    let link = socket_link(stream)?;
    shared
        .serve_link(link, || {
            // The session's reads wait as long as the link lasts.
            if let Err(e) = timed.set_read_timeout(None) {
                log!("can't clear the handshake's timeout: {e}");
            }
        })
        .map(drop)
        .map_err(io::Error::from)
}

/// A link over a connected socket. The writer's drop is the hang-up the
/// peer reads as EOF: the reader's copy of the socket would hold it open.
pub fn socket_link(stream: UnixStream) -> io::Result<Link> {
    struct Writer(UnixStream);
    impl io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }
    impl Drop for Writer {
        fn drop(&mut self) {
            let _ = self.0.shutdown(std::net::Shutdown::Write);
        }
    }
    Ok(Link::new(stream.try_clone()?, Writer(stream)))
}

/// Binds `socket`, in place of a file left there, for this user alone.
fn bind(socket: &std::path::Path) -> io::Result<UnixListener> {
    match fs::remove_file(socket) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}
