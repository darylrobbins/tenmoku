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
}

/// Where a server is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerStatus {
    /// From launch until it's ready.
    Starting,
    /// Gone for good: `None` after a requested stop, otherwise why it
    /// stopped. Its thread has ended or is about to.
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
