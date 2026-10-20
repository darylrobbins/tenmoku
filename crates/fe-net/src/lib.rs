//! HTTPS requests to the hosts the app grants (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`, § 6).
//!
//! [`Client`] sends one request and streams its response's body as it
//! arrives. It reaches only the hosts its [`Grants`] name (`net:<host>`, or
//! `net:*` for any): any other is [`NetError::HostNotAllowed`] before a DNS
//! lookup or a socket. URLs are `https`, except `http` to `127.0.0.1` or
//! `[::1]`, for tests.
//!
//! The stack is hyper's HTTP/1.1 client over `async-io` (through
//! `smol-hyper`), and rustls with the ring provider (through
//! `futures-rustls`), certificates checked by the operating system's
//! verifier and trust store (`rustls-platform-verifier`), so roots a
//! company installs work. The futures need no particular executor: the
//! connection is driven by whoever polls the response and its body.
//!
//! In this version:
//!
//! - each request opens a connection of its own and closes it when the
//!   body ends or is dropped: there is no pool;
//! - redirects aren't followed (a 3xx is returned as it is), cookies aren't
//!   kept, no proxy is used and no compression is asked for;
//! - connecting (DNS, TCP and TLS) times out after 10 s, and a read after
//!   120 s without a byte; nothing bounds the whole reply, which may stream
//!   for minutes.
//!
//! This crate must never depend on GPUI.

mod body;
mod error;
mod stream;
mod target;

use std::fmt;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::future::{Either, select};
use futures_rustls::rustls::{self, ClientConfig};
use http::header::{CONNECTION, CONTENT_LENGTH, HOST, HeaderValue, TRANSFER_ENCODING};
use http::{HeaderMap, Method, StatusCode};
use http_body_util::Full;
use hyper::body::Bytes;
use kiln_command::Grants;
use rustls_platform_verifier::BuilderVerifierExt;
use smol_hyper::rt::FuturesIo;

pub use body::Body;
pub use error::NetError;

use body::Driver;
use stream::Stream;
use target::{Allowlist, Target};

/// How long connecting may take: resolving, TCP and the TLS handshake.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read may wait for the server's next bytes, by default.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// An HTTP response whose body is still arriving.
pub struct Response {
    pub status: StatusCode,
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

/// Sends HTTPS requests to the granted hosts. Cloning it is cheap, and the
/// clones share the TLS configuration.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
    idle_timeout: Duration,
}

struct Inner {
    allowlist: Allowlist,
    /// Made by the first `https` request.
    tls: OnceLock<Result<Arc<ClientConfig>, String>>,
}

impl Client {
    /// A client that reaches the hosts in `grants`' `net:` capabilities.
    pub fn new(grants: &Grants) -> Client {
        Client {
            inner: Arc::new(Inner {
                allowlist: Allowlist::new(grants),
                tls: OnceLock::new(),
            }),
            idle_timeout: IDLE_TIMEOUT,
        }
    }

    /// Fails a read that waits `timeout` for the server's next bytes (while
    /// the response's head or its body arrives), instead of
    /// [`IDLE_TIMEOUT`].
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Client {
        self.idle_timeout = timeout;
        self
    }

    /// Sends `method` to `url` with `headers` and `body`, resolving once the
    /// response's status and headers have arrived. `Host` is set from the
    /// URL and `Connection: close` is sent; the framing is the body's own
    /// `Content-Length`, whatever `Content-Length` or `Transfer-Encoding`
    /// `headers` hold.
    ///
    /// The scheme and host are checked here, before the future does
    /// anything: a refused URL is never resolved or connected to.
    /// Dropping the future, or the response's body, closes the connection.
    pub fn request(
        &self,
        method: Method,
        url: &str,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<Response, NetError>> + Send + 'static {
        let target = Target::parse(url, &self.inner.allowlist);
        let inner = Arc::clone(&self.inner);
        let idle_timeout = self.idle_timeout;
        async move {
            let target = target?;
            let request = build(method, &target, headers, body)?;
            log::debug!(
                "fe-net: {} {}:{}{}",
                request.method(),
                target.host,
                target.port,
                target.path
            );
            let (mut sender, conn) = timeout(CONNECT_TIMEOUT, async {
                let stream = Stream::connect(&target, idle_timeout, || inner.tls_config()).await?;
                hyper::client::conn::http1::handshake(FuturesIo::new(stream))
                    .await
                    .map_err(NetError::from_hyper)
            })
            .await?;
            let mut driver = Driver::new(conn);
            let mut sent = pin!(sender.send_request(request));
            let response = poll_fn(|cx| {
                driver.drive(cx);
                sent.as_mut().poll(cx).map_err(|error| driver.error(error))
            })
            .await?;
            let (parts, incoming) = response.into_parts();
            Ok(Response {
                status: parts.status,
                headers: parts.headers,
                body: Body::new(driver, incoming),
            })
        }
    }
}

impl Inner {
    fn tls_config(&self) -> Result<Arc<ClientConfig>, NetError> {
        self.tls
            .get_or_init(|| {
                tls_config()
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(NetError::Tls)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("allowlist", &self.inner.allowlist)
            .finish_non_exhaustive()
    }
}

/// rustls with ring, TLS 1.2 and 1.3, the platform's verifier, HTTP/1.1.
fn tls_config() -> Result<ClientConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// The request hyper sends: origin-form, so the path and query only.
fn build(
    method: Method,
    target: &Target,
    mut headers: HeaderMap,
    body: Vec<u8>,
) -> Result<http::Request<Full<Bytes>>, NetError> {
    let mut request = http::Request::builder()
        .method(method)
        .uri(target.path.as_str())
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| NetError::Http(error.to_string()))?;
    let host = HeaderValue::from_str(&target.authority)
        .map_err(|error| NetError::Http(format!("{}: {error}", target.authority)))?;
    // hyper frames the request by these when they're set: the body's
    // length is what's sent.
    headers.remove(CONTENT_LENGTH);
    headers.remove(TRANSFER_ENCODING);
    headers.insert(HOST, host);
    headers.insert(CONNECTION, HeaderValue::from_static("close"));
    *request.headers_mut() = headers;
    Ok(request)
}

/// `future`, or [`NetError::Timeout`] after `duration`.
async fn timeout<T>(
    duration: Duration,
    future: impl Future<Output = Result<T, NetError>>,
) -> Result<T, NetError> {
    let future = pin!(future);
    match select(future, async_io::Timer::after(duration)).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err(NetError::Timeout),
    }
}
