// SPDX-License-Identifier: Apache-2.0

//! DAP's framing (docs/decisions/r1b9dw3g0g-debugger.md §2): each message is
//! a header section of `name: value` lines ended by an empty line, then a
//! JSON body of exactly `Content-Length` bytes. It borrows LSP's headers but
//! not its JSON-RPC envelope, so this is our own, not async-lsp's.
//!
//! - **Reading:** header lines up to `\r\n\r\n`; `Content-Length` matched
//!   without regard to case, every other header ignored; then the body.
//! - **Writing:** one `write_all` of header and body together, then a
//!   flush, per message.
//! - **Limits:** a body over [`MAX_BODY`] fails with a protocol error before
//!   anything is allocated for it, as does a header section over
//!   [`MAX_HEADER`].
//!
//! The same code runs over an adapter's stdio and over a TCP connection to
//! it (`crate::launcher`): both are an [`AsyncRead`] and an [`AsyncWrite`].

use std::fmt;
use std::io;

use futures::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use kiln_l10n::tr;

use crate::protocol::Message;

/// The largest body a message may have: 64 MB.
pub const MAX_BODY: usize = 64 * 1024 * 1024;

/// The largest header section, its final empty line included.
pub const MAX_HEADER: usize = 8 * 1024;

/// How much the reader asks its stream for at a time.
const READ_CHUNK: usize = 64 * 1024;

/// Why a message couldn't be read or written.
#[derive(Debug)]
pub enum TransportError {
    /// The stream failed, or closed partway through a message.
    Io(io::Error),
    /// The peer broke the framing, or sent a body that isn't a DAP
    /// message. The session can't go on.
    Protocol(ProtocolError),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Io(err) => write!(
                f,
                "{}",
                tr!("dap-error-transport-io", detail = err.to_string())
            ),
            TransportError::Protocol(err) => fmt::Display::fmt(err, f),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Io(err) => Some(err),
            TransportError::Protocol(err) => Some(err),
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(err: io::Error) -> Self {
        TransportError::Io(err)
    }
}

impl From<ProtocolError> for TransportError {
    fn from(err: ProtocolError) -> Self {
        TransportError::Protocol(err)
    }
}

/// How a peer broke the protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// `Content-Length` was over [`MAX_BODY`].
    TooLarge { length: u64 },
    /// The header section ended without a `Content-Length`.
    NoContentLength,
    /// A header line that isn't `name: value` in UTF-8, or a
    /// `Content-Length` that isn't a number.
    BadHeader,
    /// The header section ran past [`MAX_HEADER`] without ending.
    HeaderTooLong,
    /// The body isn't a DAP message: the JSON error, as text.
    BadMessage(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            ProtocolError::TooLarge { length } => tr!(
                "dap-error-too-large",
                bytes = length.to_string(),
                megabytes = MAX_BODY / (1024 * 1024)
            ),
            ProtocolError::NoContentLength => tr!("dap-error-no-content-length"),
            ProtocolError::BadHeader => tr!("dap-error-bad-header"),
            ProtocolError::HeaderTooLong => tr!("dap-error-header-too-long"),
            ProtocolError::BadMessage(why) => tr!("dap-error-bad-message", why = why.as_str()),
        };
        write!(f, "{text}")
    }
}

impl std::error::Error for ProtocolError {}

/// Reads framed messages from a byte stream.
///
/// [`read_body`](Self::read_body) is cancel-safe: bytes read before its
/// future is dropped stay buffered for the next call, and nothing else is
/// added, so it can sit in a `select` beside other work.
pub struct FrameReader<R> {
    stream: R,
    /// Bytes read and not yet returned.
    buffer: Vec<u8>,
    /// The body length of the message being read, once its headers are in.
    body_len: Option<usize>,
    /// Where each read lands before it is added to `buffer`.
    scratch: Option<Box<[u8]>>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(stream: R) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            body_len: None,
            scratch: None,
        }
    }

    /// The next message's body, or `None` if the stream ended cleanly
    /// between messages. A stream ending inside a message is an
    /// [`UnexpectedEof`](io::ErrorKind::UnexpectedEof) error.
    pub async fn read_body(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        loop {
            if self.body_len.is_none() {
                self.body_len = parse_headers(&mut self.buffer)?;
            }
            if let Some(len) = self.body_len
                && self.buffer.len() >= len
            {
                self.body_len = None;
                let rest = self.buffer.split_off(len);
                return Ok(Some(std::mem::replace(&mut self.buffer, rest)));
            }
            // Read into scratch space and only then keep what came, so a
            // read cancelled at its await leaves `buffer` as it was. Only
            // what's still missing of a known body is asked for.
            let want = match self.body_len {
                Some(len) => (len - self.buffer.len()).min(READ_CHUNK),
                None => READ_CHUNK,
            };
            let scratch = self
                .scratch
                .get_or_insert_with(|| vec![0; READ_CHUNK].into_boxed_slice());
            let read = self.stream.read(&mut scratch[..want]).await?;
            self.buffer.extend_from_slice(&scratch[..read]);
            if read == 0 {
                if self.buffer.is_empty() && self.body_len.is_none() {
                    return Ok(None);
                }
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
        }
    }

    /// The next message, parsed; `None` at a clean end of stream.
    pub async fn read(&mut self) -> Result<Option<Message>, TransportError> {
        let Some(body) = self.read_body().await? else {
            return Ok(None);
        };
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|err| ProtocolError::BadMessage(err.to_string()).into())
    }
}

/// If `buffer` starts with a whole header section, removes it and returns
/// its `Content-Length`. `None` while more bytes are needed.
fn parse_headers(buffer: &mut Vec<u8>) -> Result<Option<usize>, ProtocolError> {
    let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
        if buffer.len() >= MAX_HEADER {
            return Err(ProtocolError::HeaderTooLong);
        }
        return Ok(None);
    };
    if end + 4 > MAX_HEADER {
        return Err(ProtocolError::HeaderTooLong);
    }
    let headers = std::str::from_utf8(&buffer[..end]).map_err(|_| ProtocolError::BadHeader)?;
    let mut length = None;
    for line in headers.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or(ProtocolError::BadHeader)?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            let value: u64 = value.trim().parse().map_err(|_| ProtocolError::BadHeader)?;
            if value > MAX_BODY as u64 {
                return Err(ProtocolError::TooLarge { length: value });
            }
            length = Some(value as usize);
        }
    }
    let length = length.ok_or(ProtocolError::NoContentLength)?;
    buffer.drain(..end + 4);
    Ok(Some(length))
}

/// Writes framed messages to a byte stream.
pub struct FrameWriter<W> {
    stream: W,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(stream: W) -> Self {
        Self { stream }
    }

    /// Writes `body` as one message: header and body in one `write_all`,
    /// then a flush.
    pub async fn write_body(&mut self, body: &[u8]) -> io::Result<()> {
        let mut frame = Vec::with_capacity(body.len() + 32);
        frame.extend_from_slice(b"Content-Length: ");
        frame.extend_from_slice(body.len().to_string().as_bytes());
        frame.extend_from_slice(b"\r\n\r\n");
        frame.extend_from_slice(body);
        self.stream.write_all(&frame).await?;
        self.stream.flush().await
    }

    /// Writes `message`, serialised.
    pub async fn write(&mut self, message: &Message) -> io::Result<()> {
        let body = serde_json::to_vec(message).map_err(io::Error::other)?;
        self.write_body(&body).await
    }

    /// Closes the stream.
    pub async fn close(&mut self) -> io::Result<()> {
        self.stream.close().await
    }
}
