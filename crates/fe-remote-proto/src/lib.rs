//! The remote development wire protocol: a 12-byte frame header, postcard
//! bodies, and the build id the handshake compares
//! (docs/decisions/j8wvgszah0-remote-development.md § 2).
//!
//! Client and server are always one build (§ 2.5), so a body type may change
//! in any release with no compatibility rule. The multiplexer, credits and
//! the handshake's state machine come later in phase 2; this crate is the
//! frames and the types they carry.

mod control;
mod frame;
mod fs;
mod path;
mod search;
mod watch;

pub use control::{
    ControlOp, Credit, Hello, IoError, IoKind, Location, OpenTarget, Opened, Refused, Resume,
    SessionToken, Status, StreamOffset, StreamResume, Welcome,
};
pub use frame::{
    CHUNK, FrameError, HEADER, Header, Kind, MAX_BODY, SERVER_ORIGINATED, body, decode_all,
    decode_frame, decode_header, encode, encode_header, encode_raw, read_frame,
};
pub use fs::{DirEntry, DiskStamp, EntryId, EntryKind, FsData, FsOp, Stat, Time, WriteExpect};
pub use path::PathBytes;
pub use search::{
    FileMatches, LineMatch, PathFilter, SearchData, SearchOp, SearchOptions, SearchQuery,
    SearchSummary,
};
pub use watch::{FsChange, WatchOp};

/// The protocol version. Both peers must send the same one (§ 2.5).
pub const PROTOCOL: u32 = 1;

/// This build's id, `<version>+<commit>[-dirty]`, from the build script.
/// Both peers must send the same one (§ 2.5).
pub const BUILD: &str = env!("TENMOKU_BUILD");

/// The channel numbers of § 2.2.
pub mod chan {
    /// The handshake, pings, `Open`, session resume and shutdown.
    pub const CONTROL: u16 = 0;
    /// [`crate::FsOp`] requests.
    pub const FS: u16 = 1;
    /// [`crate::WatchOp`] subscriptions.
    pub const WATCH: u16 = 2;
    /// [`crate::SearchOp`] requests.
    pub const SEARCH: u16 = 3;
    /// Git (phase 8).
    pub const GIT: u16 = 4;
    /// Process control (phase 6).
    pub const PROCESS: u16 = 5;
    /// The first channel `Open` hands out. Each language server, adapter,
    /// pty, process and forwarded connection gets its own.
    pub const FIRST_DYNAMIC: u16 = 64;
}

pub type Result<T, E = FrameError> = std::result::Result<T, E>;
