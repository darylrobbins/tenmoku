//! The seam to HTTP: one call, a `POST` whose response body streams.

use std::fmt;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use http::HeaderMap;

/// Sends a request and streams its response. `fe-net` implements it over
/// HTTPS; tests implement it over recorded bytes.
pub trait Transport: Send + Sync {
    /// `POST`s `body` to `url` with `headers`, resolving once the response's
    /// status and headers have arrived.
    ///
    /// Dropping the returned [`Response`]'s `body` must close the connection
    /// (or at least stop reading it and never reuse it): that is how a chat
    /// cancels, and the server stops generating when it sees the close.
    fn post(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Response, TransportError>>;
}

/// A response body, as chunks of bytes split wherever the network split
/// them. Dropping it closes the connection.
pub type Body = BoxStream<'static, Result<Vec<u8>, TransportError>>;

/// An HTTP response whose body is still arriving.
pub struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Body,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

/// The connection failed: it couldn't be made, or it broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> TransportError {
        TransportError {
            message: message.into(),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransportError {}
