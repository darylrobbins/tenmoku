//! Channel 0's bodies, and the ones every channel shares: `End`'s status and
//! `Credit` (§ 2.2, § 2.4, § 2.5).

use crate::PathBytes;
use serde::{Deserialize, Serialize};
use std::{fmt, io};

/// The client's first frame (§ 2.5). The server refuses it unless
/// `protocol` and `build` are its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub build: String,
    /// The credit, in body bytes, the client grants each channel (§ 2.4).
    pub window: u32,
    /// Re-attaches to a session whose link dropped (§ 7).
    pub resume: Option<Resume>,
    pub location: Location,
}

/// The server's answer to a [`Hello`] it accepts. The client closes the link
/// unless `protocol` and `build` are its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol: u32,
    pub build: String,
    /// The credit the server grants each channel.
    pub window: u32,
    pub session: SessionToken,
    /// Where each stream channel resumes from, on a resume; else empty.
    pub streams: Vec<StreamResume>,
}

/// The server's answer to a [`Hello`] it refuses, before it closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    pub reason: String,
}

/// The project a session is for: `ProjectLocation::Remote` as the host sees
/// it (§ 6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// The destination as the user typed it, such as `neon` or
    /// `me@box:2222`, for the server's log.
    pub destination: String,
    /// The project root, absolute on the host.
    pub path: PathBytes,
}

/// A session's handle, for re-attaching to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionToken(pub [u8; 16]);

/// `Hello.resume`: the session to re-attach to, and the next byte the
/// client expects on each stream channel (§ 7, "Resumable streams").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub session: SessionToken,
    pub streams: Vec<StreamOffset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamOffset {
    pub chan: u16,
    pub recv_next: u64,
}

/// One of `Welcome.streams`: where the server resumes a stream channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamResume {
    /// The server expects this byte next. Each side resends from the
    /// other's `recv_next`.
    From(StreamOffset),
    /// The server's buffer passed its cap while the link was down, so it
    /// can't resend: `lost` bytes are gone, and the channel's overflow rule
    /// applies.
    Gap { chan: u16, lost: u64 },
}

/// A `Request` body on channel 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlOp {
    /// Opens a dynamic channel. The reply is one `Data` frame with
    /// [`Opened`], then `End`.
    Open(OpenTarget),
}

/// What an `Open` connects a new channel to. Later phases add language
/// servers, adapters, ptys and processes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenTarget {
    /// A TCP connection from the host (§ 8).
    Tcp { host: String, port: u16 },
}

/// The `Data` body on channel 0: the channel an `Open` made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub chan: u16,
}

/// The `Credit` body: body bytes the receiver returns to the sender on the
/// frame's channel (§ 2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credit {
    pub bytes: u32,
}

/// The `End` body: how a request ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Ok,
    /// The request was cancelled (`Cancel`).
    Cancelled,
    /// The link dropped while the request was in flight. The client gives
    /// this to its own requests; a server never sends it (§ 7).
    LinkLost,
    Error(IoError),
}

/// A failure on the host, kept apart from the link's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoError {
    pub kind: IoKind,
    pub message: String,
}

/// The `io::ErrorKind`s the client acts on. Anything else is `Other`, with
/// the host's message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IoKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    InvalidInput,
    InvalidData,
    Unsupported,
    Other,
}

impl From<io::ErrorKind> for IoKind {
    fn from(kind: io::ErrorKind) -> IoKind {
        use io::ErrorKind as K;
        match kind {
            K::NotFound => IoKind::NotFound,
            K::PermissionDenied => IoKind::PermissionDenied,
            K::AlreadyExists => IoKind::AlreadyExists,
            K::NotADirectory => IoKind::NotADirectory,
            K::IsADirectory => IoKind::IsADirectory,
            K::DirectoryNotEmpty => IoKind::DirectoryNotEmpty,
            K::InvalidInput => IoKind::InvalidInput,
            K::InvalidData => IoKind::InvalidData,
            K::Unsupported => IoKind::Unsupported,
            _ => IoKind::Other,
        }
    }
}

impl From<IoKind> for io::ErrorKind {
    fn from(kind: IoKind) -> io::ErrorKind {
        use io::ErrorKind as K;
        match kind {
            IoKind::NotFound => K::NotFound,
            IoKind::PermissionDenied => K::PermissionDenied,
            IoKind::AlreadyExists => K::AlreadyExists,
            IoKind::NotADirectory => K::NotADirectory,
            IoKind::IsADirectory => K::IsADirectory,
            IoKind::DirectoryNotEmpty => K::DirectoryNotEmpty,
            IoKind::InvalidInput => K::InvalidInput,
            IoKind::InvalidData => K::InvalidData,
            IoKind::Unsupported => K::Unsupported,
            IoKind::Other => K::Other,
        }
    }
}

impl From<&io::Error> for IoError {
    fn from(e: &io::Error) -> IoError {
        IoError {
            kind: e.kind().into(),
            message: e.to_string(),
        }
    }
}

impl From<IoError> for io::Error {
    fn from(e: IoError) -> io::Error {
        io::Error::new(e.kind.into(), e.message)
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
