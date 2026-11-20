//! The wire frame: a fixed 12-byte header, then a postcard body.
//!
//! ```text
//! u32 LE  len      bytes after this field (8 header bytes + body)
//! u8      kind     Kind
//! u8      flags    reserved, 0
//! u16 LE  chan     channel id (0 = control)
//! u32 LE  id       request id, or a ping nonce
//! [len-8] body     postcard of the kind's body type
//! ```
//!
//! The header is fixed so a router can read the channel, request and kind
//! (and count credits) without decoding the body. The body is bounded by
//! `MAX_BODY`; the decoder checks the bound before allocating.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const PROTOCOL: u32 = 1;
pub const HEADER: usize = 12;
/// Largest body a peer may send. Senders chunk well below it (`CHUNK`).
pub const MAX_BODY: usize = 1 << 20;
/// File and stream chunk size.
pub const CHUNK: usize = 64 * 1024;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    fn from_u8(b: u8) -> Option<Kind> {
        use Kind::*;
        Some(match b {
            1 => Hello,
            2 => Welcome,
            3 => Refused,
            4 => Request,
            5 => Data,
            6 => End,
            7 => Cancel,
            8 => Credit,
            9 => Ping,
            10 => Pong,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub chan: u16,
    pub id: u32,
    pub body_len: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    TooShort(u32),
    TooLong(u32),
    BadKind(u8),
    BadFlags(u8),
    Body(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FrameError {}

impl From<FrameError> for io::Error {
    fn from(e: FrameError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e)
    }
}

/// Handshake, both ways. The server refuses any protocol or build mismatch.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Hello {
    pub protocol: u32,
    pub build: String,
    /// Initial credit, in body bytes, the sender grants each channel.
    pub window: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Op {
    Echo(#[serde(with = "serde_bytes")] Vec<u8>),
    ReadFile { path: String },
    Walk { root: String },
    Bulk { bytes: u64 },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: String,
    pub dir: bool,
    pub size: u64,
    pub mtime_ns: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Body {
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    Entries(Vec<Entry>),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    Cancelled,
    Error(String),
}

pub fn encode_header(kind: Kind, chan: u16, id: u32, body_len: usize) -> [u8; HEADER] {
    assert!(body_len <= MAX_BODY, "body over MAX_BODY");
    let mut h = [0u8; HEADER];
    h[0..4].copy_from_slice(&((body_len + 8) as u32).to_le_bytes());
    h[4] = kind as u8;
    h[6..8].copy_from_slice(&chan.to_le_bytes());
    h[8..12].copy_from_slice(&id.to_le_bytes());
    h
}

/// Checks a header without trusting any of it.
pub fn decode_header(h: &[u8; HEADER]) -> Result<Header, FrameError> {
    let len = u32::from_le_bytes(h[0..4].try_into().unwrap());
    if len < 8 {
        return Err(FrameError::TooShort(len));
    }
    if len as usize - 8 > MAX_BODY {
        return Err(FrameError::TooLong(len));
    }
    let kind = Kind::from_u8(h[4]).ok_or(FrameError::BadKind(h[4]))?;
    if h[5] != 0 {
        return Err(FrameError::BadFlags(h[5]));
    }
    Ok(Header {
        kind,
        chan: u16::from_le_bytes(h[6..8].try_into().unwrap()),
        id: u32::from_le_bytes(h[8..12].try_into().unwrap()),
        body_len: len as usize - 8,
    })
}

/// One whole frame, header and postcard body.
pub fn encode<T: Serialize>(kind: Kind, chan: u16, id: u32, body: &T) -> Vec<u8> {
    let mut out = vec![0u8; HEADER];
    let mut out = postcard::to_extend(body, std::mem::take(&mut out)).expect("encode");
    let len = out.len() - HEADER;
    out[..HEADER].copy_from_slice(&encode_header(kind, chan, id, len));
    out
}

pub fn body<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, FrameError> {
    postcard::from_bytes(bytes).map_err(|e| FrameError::Body(e.to_string()))
}

/// Reads one frame. `Ok(None)` is a clean end of stream at a frame boundary.
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
    let mut b = vec![0u8; header.body_len];
    r.read_exact(&mut b)?;
    Ok(Some((header, b)))
}

pub fn write_frame(w: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    w.write_all(frame)
}

/// Decodes every frame in `bytes`, the shape a fuzz target takes: it must
/// return an error, never panic or allocate past `MAX_BODY`, on any input.
pub fn decode_all(mut bytes: &[u8]) -> Result<Vec<Header>, io::Error> {
    let mut out = Vec::new();
    while let Some((h, b)) = read_frame(&mut bytes)? {
        match h.kind {
            Kind::Hello | Kind::Welcome => drop(body::<Hello>(&b)?),
            Kind::Refused => drop(body::<String>(&b)?),
            Kind::Request => drop(body::<Op>(&b)?),
            Kind::Data => drop(body::<Body>(&b)?),
            Kind::End => drop(body::<Status>(&b)?),
            Kind::Credit => drop(body::<u32>(&b)?),
            Kind::Cancel | Kind::Ping | Kind::Pong => drop(body::<()>(&b)?),
        }
        out.push(h);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips() {
        let f = encode(Kind::Request, 2, 7, &Op::ReadFile { path: "a/b".into() });
        let hs = decode_all(&f).unwrap();
        assert_eq!(hs[0].kind, Kind::Request);
        assert_eq!((hs[0].chan, hs[0].id), (2, 7));
    }

    #[test]
    fn an_oversized_length_is_refused_before_allocating() {
        let mut h = encode_header(Kind::Data, 1, 1, 0);
        h[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_header(&h), Err(FrameError::TooLong(u32::MAX)));
    }

    #[test]
    fn truncated_frames_and_bad_kinds_are_errors() {
        let f = encode(Kind::End, 1, 1, &Status::Ok);
        assert!(decode_all(&f[..f.len() - 1]).is_err());
        assert!(decode_all(&f[..5]).is_err());
        let mut bad = f.clone();
        bad[4] = 99;
        assert!(decode_all(&bad).is_err());
    }

    /// A poor man's fuzz loop: random and mutated frames never panic.
    #[test]
    fn garbage_never_panics() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let good = encode(
            Kind::Data,
            1,
            1,
            &Body::Entries(vec![Entry {
                path: "src/main.rs".into(),
                dir: false,
                size: 10,
                mtime_ns: 1,
            }]),
        );
        for _ in 0..200_000 {
            let mut f = good.clone();
            for _ in 0..(next() % 4) {
                let i = (next() as usize) % f.len();
                f[i] = next() as u8;
            }
            let cut = (next() as usize) % (f.len() + 1);
            let _ = decode_all(&f[..cut]);
        }
    }
}
