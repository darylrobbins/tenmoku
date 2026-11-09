//! [`NetError`]: why a request failed.

use kiln_l10n::tr;

/// Why a request failed, or its body stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The URL's host is outside the grants. Returned before any DNS lookup
    /// or socket.
    HostNotAllowed(String),
    /// The URL isn't `https`, or `http` to `127.0.0.1` or `[::1]`.
    Scheme,
    /// The host couldn't be resolved or connected to.
    Connect(String),
    /// The TLS handshake failed, as when the certificate isn't trusted.
    Tls(String),
    /// The connection broke after it was made.
    Io(String),
    /// The request couldn't be made, or the server's reply isn't HTTP/1.1.
    Http(String),
    /// Connecting took longer than the connect timeout, or the server sent
    /// nothing for the idle timeout.
    Timeout,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            NetError::HostNotAllowed(host) => tr!("net-host-not-allowed", host = host),
            NetError::Scheme => tr!("net-scheme"),
            NetError::Connect(detail) => tr!("net-connect", detail = detail),
            NetError::Tls(detail) => tr!("net-tls", detail = detail),
            NetError::Io(detail) => tr!("net-io", detail = detail),
            NetError::Http(detail) => tr!("net-http", detail = detail),
            NetError::Timeout => tr!("net-timeout"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for NetError {}

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
