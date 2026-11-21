//! The wire frame (§ 2.1): a fixed 12-byte header, then a postcard body.
//!
//! ```text
//! u32 LE  len    bytes after this field: 8 header bytes + body (8 ..= 8 + MAX_BODY)
//! u8      kind   Kind
//! u8      flags  0 (reserved; non-zero is a protocol error in v1)
//! u16 LE  chan   channel (0 = control)
//! u32 LE  id     request id (SERVER_ORIGINATED set = server-originated), or ping nonce
//! [len-8] body   postcard of the kind's body type
//! ```
//!
//! The header is fixed so a demultiplexer, the credit counter or a relay can
//! read the channel, request and length without decoding the body. Nothing
//! here trusts a byte it reads: the length is checked against `MAX_BODY`
//! before anything is allocated for the body.

use crate::{
    ControlOp, Credit, FsChange, FsData, FsOp, Hello, Opened, Refused, Result, SearchData,
    SearchOp, Status, WatchOp, Welcome, chan,
};
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

pub const HEADER: usize = 12;
/// The largest body a peer may send.
pub const MAX_BODY: usize = 1 << 20;
/// What senders chunk files, byte streams and walk batches at, so that one
/// frame holds the wire for at most this many bytes.
pub const CHUNK: usize = 64 * 1024;
/// The id bit that marks a request the server started (§ 2.2).
pub const SERVER_ORIGINATED: u32 = 1 << 31;

/// The bytes `len` counts that aren't the body: kind, flags, chan and id.
const AFTER_LEN: usize = HEADER - 4;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Hello = 1,
    Welcome = 2,
    Refused = 3,
    Request = 4,
    Data = 5,
    End = 6,
    Cancel = 7,
    Credit = 8,
    Ping = 9,
    Pong = 10,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Hello,
        Kind::Welcome,
        Kind::Refused,
        Kind::Request,
        Kind::Data,
        Kind::End,
        Kind::Cancel,
        Kind::Credit,
        Kind::Ping,
        Kind::Pong,
    ];

    pub fn from_u8(b: u8) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| *k as u8 == b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub chan: u16,
    pub id: u32,
    pub body_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame length {0} is shorter than the 8 header bytes it counts")]
    TooShort(u32),
    #[error("frame length {0} is over 8 + {MAX_BODY}")]
    TooLong(u32),
    #[error("unknown frame kind {0}")]
    BadKind(u8),
    #[error("frame flags {0:#04x} aren't 0")]
    BadFlags(u8),
    #[error("{kind:?} isn't sent on channel {chan}")]
    BadChannel { kind: Kind, chan: u16 },
    #[error("frame cut short: {need} bytes needed, {have} there")]
    Truncated { need: usize, have: usize },
    #[error("{kind:?} body on channel {chan}: {message}")]
    Body {
        kind: Kind,
        chan: u16,
        message: String,
    },
    #[error("{kind:?} body on channel {chan} has {extra} bytes after it")]
    Trailing { kind: Kind, chan: u16, extra: usize },
    #[error("a body of {0} bytes is over {MAX_BODY}")]
    BodyTooLong(usize),
    #[error("encoding a body: {0}")]
    Encode(String),
}

impl From<FrameError> for io::Error {
    fn from(e: FrameError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e)
    }
}

pub fn encode_header(header: &Header) -> Result<[u8; HEADER]> {
    if header.body_len > MAX_BODY {
        return Err(FrameError::BodyTooLong(header.body_len));
    }
    let mut h = [0u8; HEADER];
    h[0..4].copy_from_slice(&((header.body_len + AFTER_LEN) as u32).to_le_bytes());
    h[4] = header.kind as u8;
    h[6..8].copy_from_slice(&header.chan.to_le_bytes());
    h[8..12].copy_from_slice(&header.id.to_le_bytes());
    Ok(h)
}

/// Checks a header without trusting any of it: the length first, so a
/// caller can allocate `body_len` once this returns.
pub fn decode_header(h: &[u8; HEADER]) -> Result<Header> {
    let len = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
    let body_len = (len as usize)
        .checked_sub(AFTER_LEN)
        .ok_or(FrameError::TooShort(len))?;
    if body_len > MAX_BODY {
        return Err(FrameError::TooLong(len));
    }
    let kind = Kind::from_u8(h[4]).ok_or(FrameError::BadKind(h[4]))?;
    if h[5] != 0 {
        return Err(FrameError::BadFlags(h[5]));
    }
    Ok(Header {
        kind,
        chan: u16::from_le_bytes([h[6], h[7]]),
        id: u32::from_le_bytes([h[8], h[9], h[10], h[11]]),
        body_len,
    })
}

/// One whole frame: the header and `body` in postcard.
pub fn encode<T: Serialize + ?Sized>(kind: Kind, chan: u16, id: u32, body: &T) -> Result<Vec<u8>> {
    let mut out = postcard::to_extend(body, vec![0u8; HEADER])
        .map_err(|e| FrameError::Encode(e.to_string()))?;
    let body_len = out.len() - HEADER;
    let header = encode_header(&Header {
        kind,
        chan,
        id,
        body_len,
    })?;
    out[..HEADER].copy_from_slice(&header);
    Ok(out)
}

/// One whole frame whose body is `body` as it is: an opaque `Data` frame on
/// a dynamic channel.
pub fn encode_raw(kind: Kind, chan: u16, id: u32, body: &[u8]) -> Result<Vec<u8>> {
    let header = encode_header(&Header {
        kind,
        chan,
        id,
        body_len: body.len(),
    })?;
    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    Ok(out)
}

/// Decodes the body of the frame `header` heads. Bytes left over after the
/// value are an error.
pub fn body<'a, T: Deserialize<'a>>(header: &Header, bytes: &'a [u8]) -> Result<T> {
    let (value, rest) = postcard::take_from_bytes(bytes).map_err(|e| FrameError::Body {
        kind: header.kind,
        chan: header.chan,
        message: e.to_string(),
    })?;
    if !rest.is_empty() {
        return Err(FrameError::Trailing {
            kind: header.kind,
            chan: header.chan,
            extra: rest.len(),
        });
    }
    Ok(value)
}

/// Splits the frame at the start of `bytes` into its header, its body and
/// the bytes after it.
pub fn decode_frame(bytes: &[u8]) -> Result<(Header, &[u8], &[u8])> {
    let Some((h, rest)) = bytes.split_first_chunk::<HEADER>() else {
        return Err(FrameError::Truncated {
            need: HEADER,
            have: bytes.len(),
        });
    };
    let header = decode_header(h)?;
    if rest.len() < header.body_len {
        return Err(FrameError::Truncated {
            need: HEADER + header.body_len,
            have: bytes.len(),
        });
    }
    let (body, rest) = rest.split_at(header.body_len);
    Ok((header, body, rest))
}

/// Reads one frame. `Ok(None)` is a clean end of stream at a frame boundary.
/// The body is allocated only once the header has passed
/// [`decode_header`], so never over [`MAX_BODY`].
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<(Header, Vec<u8>)>> {
    let mut h = [0u8; HEADER];
    let mut got = 0;
    while got < HEADER {
        match r.read(&mut h[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let header = decode_header(&h)?;
    let mut body = vec![0u8; header.body_len];
    r.read_exact(&mut body)?;
    Ok(Some((header, body)))
}

/// Decodes the header and then the typed body of every frame in `bytes`.
/// This is what the fuzz target drives: on any input it returns, an error
/// at worst, and never panics or allocates past [`MAX_BODY`] for a body.
pub fn decode_all(mut bytes: &[u8]) -> Result<Vec<Header>> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let (header, body, rest) = decode_frame(bytes)?;
        check_body(&header, body)?;
        out.push(header);
        bytes = rest;
    }
    Ok(out)
}

/// Decodes `bytes` as the body type of `header`'s kind on its channel, and
/// refuses a kind its channel doesn't carry. `Data` on a dynamic channel is
/// opaque here: only the channel's owner knows what it carries.
fn check_body(header: &Header, bytes: &[u8]) -> Result<()> {
    let bad_channel = || FrameError::BadChannel {
        kind: header.kind,
        chan: header.chan,
    };
    match (header.kind, header.chan) {
        (Kind::Hello, chan::CONTROL) => drop(body::<Hello>(header, bytes)?),
        (Kind::Welcome, chan::CONTROL) => drop(body::<Welcome>(header, bytes)?),
        (Kind::Refused, chan::CONTROL) => drop(body::<Refused>(header, bytes)?),
        (Kind::Ping | Kind::Pong, chan::CONTROL) => body::<()>(header, bytes)?,
        (Kind::Hello | Kind::Welcome | Kind::Refused | Kind::Ping | Kind::Pong, _) => {
            return Err(bad_channel());
        }
        (Kind::Request, chan::CONTROL) => drop(body::<ControlOp>(header, bytes)?),
        (Kind::Request, chan::FS) => drop(body::<FsOp>(header, bytes)?),
        (Kind::Request, chan::WATCH) => drop(body::<WatchOp>(header, bytes)?),
        (Kind::Request, chan::SEARCH) => drop(body::<SearchOp>(header, bytes)?),
        (Kind::Request, _) => return Err(bad_channel()),
        (Kind::Data, chan::CONTROL) => drop(body::<Opened>(header, bytes)?),
        (Kind::Data, chan::FS) => drop(body::<FsData>(header, bytes)?),
        (Kind::Data, chan::WATCH) => drop(body::<Vec<FsChange>>(header, bytes)?),
        (Kind::Data, chan::SEARCH) => drop(body::<SearchData>(header, bytes)?),
        (Kind::Data, c) if c >= chan::FIRST_DYNAMIC => {}
        (Kind::Data, _) => return Err(bad_channel()),
        (Kind::End, _) => drop(body::<Status>(header, bytes)?),
        (Kind::Credit, _) => drop(body::<Credit>(header, bytes)?),
        (Kind::Cancel, _) => body::<()>(header, bytes)?,
    }
    Ok(())
}
