//! `tenmoku-server --stdio` (D § 3): what ssh runs. It connects to the
//! build's daemon, starting it detached (`setsid`) if no lock is held, and
//! then splices its stdio to the socket. When ssh dies, only this process
//! dies: the daemon sees the socket close and the session's grace begins.
//!
//! Stdin and stdout are the protocol, untouched; stderr is the connection
//! log (D § 1).

use crate::Paths;
use std::fs::OpenOptions;
use std::io::{self, Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long an attach waits for a daemon on its way up.
const START_WAIT: Duration = Duration::from_secs(20);
/// Between tries at a daemon on its way up.
const RETRY: Duration = Duration::from_millis(20);
/// The splice's buffer: a frame's chunk and its header fit.
const BUFFER: usize = 80 * 1024;

/// Connects to `paths`' daemon, starting `exe daemon` if none is alive, and
/// splices this process's stdio to it until either side ends. Returns only
/// if the daemon can't be reached.
pub fn attach(paths: &Paths, exe: &Path) -> io::Result<()> {
    let socket = connect(paths, exe)?;
    splice(socket)
}

/// The socket of `paths`' daemon, once it answers.
pub fn connect(paths: &Paths, exe: &Path) -> io::Result<UnixStream> {
    paths.secure()?;
    let deadline = Instant::now() + START_WAIT;
    let mut started: Option<Child> = None;
    loop {
        if let Ok(socket) = UnixStream::connect(&paths.socket) {
            return Ok(socket);
        }
        match &mut started {
            None => {
                // A free lock means no daemon, alive or starting. Another
                // attach may start one at the same time: the daemon that
                // takes the lock serves, and the other exits.
                if paths.take_lock()?.is_some() {
                    let child = start(paths, exe)?;
                    log!("started the daemon, pid {}", child.id());
                    started = Some(child);
                }
            }
            Some(child) => {
                if let Some(status) = child.try_wait()?
                    && !status.success()
                {
                    return Err(io::Error::other(format!(
                        "the daemon exited ({status}) before it listened; its log is {}",
                        paths.log.display()
                    )));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "no daemon answered on {} within {START_WAIT:?}; its log is {}",
                    paths.socket.display(),
                    paths.log.display()
                ),
            ));
        }
        thread::sleep(RETRY);
    }
}

/// Starts `exe daemon` in a session of its own, with no terminal, its
/// stderr appended to the daemon's log.
fn start(paths: &Paths, exe: &Path) -> io::Result<Child> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&paths.log)?;
    let mut command = Command::new(exe);
    command
        .arg("daemon")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

/// Copies stdin to `socket` and `socket` to stdout until the daemon hangs
/// up or stdout closes, then exits. Stdin's end is passed on as the
/// socket's: the daemon then hangs up in turn.
fn splice(socket: UnixStream) -> io::Result<()> {
    let mut up = socket.try_clone()?;
    thread::Builder::new()
        .name("stdio-up".into())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buf = vec![0; BUFFER];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if up.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = up.shutdown(Shutdown::Write);
        })?;
    let mut down = socket;
    let mut stdout = io::stdout().lock();
    let mut buf = vec![0; BUFFER];
    loop {
        match down.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if stdout
                    .write_all(&buf[..n])
                    .and_then(|()| stdout.flush())
                    .is_err()
                {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                log!("the daemon's socket failed: {e}");
                break;
            }
        }
    }
    std::process::exit(0)
}
