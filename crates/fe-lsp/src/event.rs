//! What fe-lsp reports: events through the manager's sink, and request
//! errors.

/// Identifies one server instance started by an [`LspManager`](crate::LspManager).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerId(pub(crate) u64);

/// Sent to the manager's sink, on the server's thread.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum LspEvent {
    /// A server's state changed.
    Status {
        server: ServerId,
        name: String,
        status: ServerStatus,
    },
    /// The server asked for a message to be shown (`window/showMessage`).
    ShowMessage {
        server: ServerId,
        name: String,
        typ: lsp_types::MessageType,
        message: String,
    },
}

/// Where a server is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerStatus {
    /// From launch until it's ready, with no counted progress open.
    Starting,
    /// Not ready yet, with work-done progress open: the title of the
    /// latest begun.
    Indexing { title: String },
    /// Ready (§3.4), until the session ends: requests go to it.
    Ready,
    /// The *n*th crash within 3 minutes ended its session; it starts again
    /// after a backoff (§3.5).
    Restarting(u32),
    /// Gone for good: `None` after a requested stop, otherwise why it
    /// stopped (it couldn't launch, or crashed too often). Its thread has
    /// ended or is about to.
    Stopped(Option<String>),
}

/// Why a request got no answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LspError {
    /// The server isn't running, stopped, or went away before answering.
    #[error("the language server isn't running")]
    ServerDown,
    /// The request was cancelled (`RequestCancelled`).
    #[error("the request was cancelled")]
    Cancelled,
    #[error("the request timed out")]
    Timeout,
    /// The server can't answer this; something else should.
    #[error("{0}")]
    Unavailable(String),
    /// The server says its answer would be stale (`ContentModified`,
    /// `ServerCancelled`): ask again.
    #[error("the server asked for the request to be retried")]
    Retry,
    /// The server answered with an error.
    #[error("{message} ({code})")]
    Server { code: i32, message: String },
    /// The reply couldn't be read.
    #[error("bad reply: {0}")]
    Protocol(String),
}

/// [`LspError::Unavailable`]'s reasons for a request on a document (§4.1).
/// The buffer moved on from the version the request was computed on.
pub(crate) const BUFFER_CHANGED: &str = "buffer changed";
/// The document is over the size limit, so the server doesn't have it.
pub(crate) const TOO_LARGE: &str = "too large for the language server";
/// The document isn't open on a language server.
pub(crate) const NOT_OPEN: &str = "not open on a language server";

impl LspError {
    pub(crate) fn from_lsp(err: async_lsp::Error) -> Self {
        match err {
            async_lsp::Error::ServiceStopped | async_lsp::Error::Eof => Self::ServerDown,
            async_lsp::Error::Response(err) => match err.code {
                async_lsp::ErrorCode::REQUEST_CANCELLED => Self::Cancelled,
                async_lsp::ErrorCode::CONTENT_MODIFIED | async_lsp::ErrorCode::SERVER_CANCELLED => {
                    Self::Retry
                }
                code => Self::Server {
                    code: code.0,
                    message: err.message,
                },
            },
            err => Self::Protocol(err.to_string()),
        }
    }
}

/// How much of each server's stderr and `window/logMessage` is kept.
pub(crate) const LOG_BYTES: usize = 64 * 1024;

/// The last [`LOG_BYTES`] a server wrote to stderr or logged, across its
/// sessions, read with [`LspManager::log`](crate::LspManager::log).
#[derive(Default)]
pub(crate) struct LogRing(std::collections::VecDeque<u8>);

pub(crate) type SharedLog = std::sync::Arc<std::sync::Mutex<LogRing>>;

impl LogRing {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(LOG_BYTES)..];
        let over = (self.0.len() + bytes.len()).saturating_sub(LOG_BYTES);
        self.0.drain(..over);
        self.0.extend(bytes);
    }

    /// What's kept, from its first whole character on.
    pub(crate) fn text(&self) -> String {
        let bytes: Vec<u8> = self.0.iter().copied().collect();
        let start = bytes
            .iter()
            .position(|&b| b & 0xc0 != 0x80)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[start..]).into_owned()
    }
}
