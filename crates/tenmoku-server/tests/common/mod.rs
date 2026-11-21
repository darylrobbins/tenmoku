//! A daemon run in the test's process on a manual clock, the real
//! `--stdio` binary, and clients of both.

#![allow(dead_code, unreachable_pub)]

use fe_remote_proto::{
    BUILD, Config as MuxConfig, HandshakeError, Link, Location, Mux, PathBytes, Resume, Welcome,
};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::{self, JoinHandle};
use tempfile::TempDir;
use tenmoku_server::clock::{Clock, ManualClock};
use tenmoku_server::{Config, Daemon, Paths};

pub const BIN: &str = env!("CARGO_BIN_EXE_tenmoku-server");

/// A runtime dir and a home of the test's own, in `/tmp` so a socket's
/// path stays well under the limit and never falls back to the shared
/// `/tmp/tenmoku-server-<uid>/`.
pub struct Host {
    _tmp: TempDir,
    pub runtime: PathBuf,
    pub home: PathBuf,
}

impl Host {
    pub fn new() -> Host {
        let tmp = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let runtime = tmp.path().join("r");
        let home = tmp.path().join("h");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::create_dir(&home).unwrap();
        Host {
            _tmp: tmp,
            runtime,
            home,
        }
    }

    pub fn paths(&self) -> Paths {
        Paths::new(BUILD, Some(&self.runtime), Some(&self.home)).unwrap()
    }

    /// `tenmoku-server --stdio` on this host.
    pub fn stdio(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .arg("--stdio")
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("HOME", &self.home);
        command
    }
}

pub fn location() -> Location {
    Location {
        destination: "neon".into(),
        path: PathBytes(b"/home/me/app".to_vec()),
    }
}

/// A client of this build that doesn't ping.
pub fn client_config() -> MuxConfig {
    MuxConfig {
        heartbeat: None,
        ..MuxConfig::default()
    }
}

pub fn handshake(
    link: Link,
    config: MuxConfig,
    resume: Option<Resume>,
) -> Result<(Mux, Welcome), HandshakeError> {
    Mux::client(link, config, location(), resume)
}

/// A link straight to the daemon's socket, as `--stdio` would splice it.
pub fn socket_link(paths: &Paths) -> Link {
    tenmoku_server::socket_link(UnixStream::connect(&paths.socket).unwrap()).unwrap()
}

/// A daemon serving `host` in this process, on a manual clock.
pub struct Running {
    pub daemon: Daemon,
    pub clock: ManualClock,
    pub thread: JoinHandle<()>,
}

pub fn run_daemon(host: &Host, config: Config) -> Running {
    let (daemon, clock) = claim_daemon(host, config);
    let running = daemon.clone();
    let thread = thread::spawn(move || running.run());
    Running {
        daemon,
        clock,
        thread,
    }
}

/// A daemon for `host` on a manual clock that isn't running: nothing ends
/// a session unless something it's asked to do does.
pub fn claim_daemon(host: &Host, config: Config) -> (Daemon, ManualClock) {
    let clock = ManualClock::new();
    let config = Config {
        clock: Clock::Manual(clock.clone()),
        ..config
    };
    let daemon = Daemon::claim(host.paths(), config)
        .unwrap()
        .expect("no other daemon holds the lock");
    (daemon, clock)
}

/// A client of `daemon` over an in-process pipe, served on a thread.
pub fn serve_client(
    daemon: &Daemon,
    resume: Option<Resume>,
) -> (
    Mux,
    Welcome,
    JoinHandle<Result<fe_remote_proto::SessionToken, HandshakeError>>,
) {
    let (client_end, server_end) = Link::pipe().unwrap();
    let serving = daemon.clone();
    let served = thread::spawn(move || serving.serve(server_end));
    let (mux, welcome) = handshake(client_end, client_config(), resume).unwrap();
    (mux, welcome, served)
}

/// The lock is held by a live process.
pub fn lock_held(paths: &Paths) -> bool {
    paths.take_lock().unwrap().is_none()
}

pub fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}
