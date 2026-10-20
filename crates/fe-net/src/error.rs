//! [`NetError`]: why a request failed.

/// Why a request failed, or its body stopped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// The URL's host is outside the grants. Returned before any DNS lookup
    /// or socket.
    #[error("{0} is not a host this client may reach")]
    HostNotAllowed(String),
    /// The URL isn't `https`, or `http` to `127.0.0.1` or `[::1]`.
    #[error("only https URLs are allowed (http only to 127.0.0.1 or [::1])")]
    Scheme,
    /// The host couldn't be resolved or connected to.
    #[error("couldn't connect: {0}")]
    Connect(String),
    /// The TLS handshake failed, as when the certificate isn't trusted.
    #[error("TLS failed: {0}")]
    Tls(String),
    /// The connection broke after it was made.
    #[error("the connection broke: {0}")]
    Io(String),
    /// The request couldn't be made, or the server's reply isn't HTTP/1.1.
    #[error("HTTP error: {0}")]
    Http(String),
    /// Connecting took longer than the connect timeout, or the server sent
    /// nothing for the idle timeout.
    #[error("timed out")]
    Timeout,
}

impl NetError {
    /// The error a hyper error stands for: [`NetError::Io`] when the
    /// connection broke under it, [`NetError::Http`] otherwise.
    pub(crate) fn from_hyper(error: hyper::Error) -> NetError {
        if error.is_timeout() {
            return NetError::Timeout;
        }
        let mut source = std::error::Error::source(&error);
        while let Some(cause) = source {
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                if io.kind() == std::io::ErrorKind::TimedOut {
                    return NetError::Timeout;
                }
                return NetError::Io(format!("{error}: {io}"));
            }
            source = cause.source();
        }
        match std::error::Error::source(&error) {
            Some(cause) => NetError::Http(format!("{error}: {cause}")),
            None => NetError::Http(error.to_string()),
        }
    }
}
