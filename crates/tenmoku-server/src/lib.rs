//! The remote development server (docs/decisions/j8wvgszah0-remote-development.md
//! § 3), Linux and macOS only.
//!
//! `tenmoku-server daemon` holds the sessions of one host, user and build
//! ([`Daemon`]). `tenmoku-server --stdio`, which ssh runs, connects to it,
//! starting it if need be, and splices its stdio to the daemon's socket
//! ([`stdio::attach`]). Each link is one client window's session. A session
//! whose link drops waits [`Config::grace`] for its client, then its process
//! groups are torn down, and the daemon exits [`Config::linger`] after its
//! last session.
//!
//! [`serve`] runs one session over any link, so a client's tests can run a
//! server in-process without the binary.
//!
//! Control is the only service so far: pings are answered, and `Open` and
//! the fixed service channels end with an `Unsupported` status.

#![cfg(unix)]

macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log_line(format_args!($($arg)*))
    };
}

pub mod clock;
mod daemon;
mod paths;
mod server;
pub mod stdio;
#[cfg(feature = "test-support")]
pub mod testing;

pub use daemon::{Daemon, socket_link};
pub use paths::Paths;
pub use server::{SessionInfo, Snapshot, TERM_WAIT, TornDown};

use clock::Clock;
use fe_remote_proto::{HandshakeError, Link, SessionToken};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a session waits for its client after the link drops (D § 3).
pub const GRACE: Duration = Duration::from_secs(10 * 60);
/// How long the daemon stays after its last session ends (D § 3).
pub const LINGER: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct Config {
    /// The protocol, build and window the handshake goes by. The server
    /// doesn't ping: the client does, and the server answers (D § 2.5).
    pub mux: fe_remote_proto::Config,
    pub grace: Duration,
    pub linger: Duration,
    /// The time the grace, the teardown and the linger read.
    pub clock: Clock,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            mux: fe_remote_proto::Config {
                heartbeat: None,
                ..fe_remote_proto::Config::default()
            },
            grace: GRACE,
            linger: LINGER,
            clock: Clock::System,
        }
    }
}

/// Serves one session over `link` until the link is lost, and then ends it
/// at once, for nothing else can reach it to resume it. Refuses a `Hello`
/// of another protocol or build, as the daemon does.
pub fn serve(link: Link, config: Config) -> Result<SessionToken, HandshakeError> {
    let shared = server::Shared::new(Config {
        grace: Duration::ZERO,
        ..config
    });
    let served = shared.serve_link(link, || {});
    shared.reap(None);
    served
}

#[doc(hidden)]
pub fn log_line(args: fmt::Arguments<'_>) {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    eprintln!(
        "tenmoku-server[{}] {}.{:03}: {args}",
        std::process::id(),
        at.as_secs(),
        at.subsec_millis()
    );
}
