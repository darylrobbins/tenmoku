//! The editor's network access for plugins: [`FeNet`], kiln-plugin's [`NetBackend`] over an
//! fe-net [`Client`].
//!
//! kiln-plugin checks each URL against the plugin's grants and follows redirects; this sends
//! one request at a time. The client is made from the plugin's own grants, so fe-net's check
//! before DNS agrees with kiln's. fe-net keeps no cookies, uses no proxy and returns a redirect
//! as it is; the headers a plugin sends are filtered here (see [`DROPPED_REQUEST_HEADERS`]),
//! and `Set-Cookie` is dropped from what it receives.
//!
//! fe-net reports no peer, so the peer this gives kiln is the host fe-net connects to: the
//! URL's host as fe-net's own parser reads it, which is also the name its TLS handshake
//! verifies the certificate against. A URL the two parsers read differently is refused.
//! fe-net resolves that name itself when it connects: the connection is to the name checked,
//! not pinned to an address.

use std::future::Future;
use std::pin::pin;
use std::time::Duration;

use fe_net::{Client, NetError};
use futures::StreamExt as _;
use futures::future::{Either, select};
use http::header::{HeaderName, HeaderValue};
use http::{HeaderMap, Uri};
use kiln_command::Grants;
use kiln_plugin::bindings::Error;
use kiln_plugin::bindings::kiln::extension::net::Method;
use kiln_plugin::net::{NetBackend, Outgoing, Received};

/// Request headers a plugin may not send: those that frame or route the request, which
/// fe-net sets, cookies, and `Accept-Encoding`, since nothing decodes a compressed body.
pub const DROPPED_REQUEST_HEADERS: [&str; 14] = [
    "accept-encoding",
    "connection",
    "content-length",
    "cookie",
    "cookie2",
    "expect",
    "host",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Response headers a plugin isn't shown: cookies, which nothing keeps.
const DROPPED_RESPONSE_HEADERS: [&str; 2] = ["set-cookie", "set-cookie2"];

/// Sends a plugin's requests through fe-net.
#[derive(Clone, Debug)]
pub struct FeNet {
    client: Client,
}

impl FeNet {
    /// A backend that reaches the hosts in the plugin's `grants`.
    pub fn new(grants: &Grants) -> Self {
        Self {
            client: Client::new(grants),
        }
    }
}

impl NetBackend for FeNet {
    fn send(&mut self, request: &Outgoing) -> Result<Received, Error> {
        let peer = peer(&request.url)?;
        let headers = request_headers(&request.headers)?;
        let response = self.client.request(
            method(request.method),
            &request.url,
            headers,
            request.body.clone().unwrap_or_default(),
        );
        let max_body_bytes = request.max_body_bytes;
        let url = request.url.as_str();
        async_io::block_on(timeout(request.timeout, url, async move {
            let response = response.await.map_err(|error| net_error(url, error))?;
            let mut body = Vec::new();
            let mut chunks = response.body;
            while let Some(chunk) = chunks.next().await {
                let chunk = chunk.map_err(|error| net_error(url, error))?;
                if body.len() + chunk.len() > max_body_bytes {
                    return Err(Error::Failed(format!(
                        "{url}'s response is over {max_body_bytes} bytes"
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Received {
                status: response.status.as_u16(),
                headers: response_headers(&response.headers),
                body,
                peer,
            })
        }))
    }
}

/// The host fe-net connects to for `url`: lower case, an IPv6 address without its brackets.
fn peer(url: &str) -> Result<String, Error> {
    let uri: Uri = url
        .parse()
        .map_err(|error| Error::InvalidArgument(format!("{url:?} isn't a URL: {error}")))?;
    let host = uri
        .host()
        .ok_or_else(|| Error::InvalidArgument(format!("{url:?} has no host")))?
        .to_ascii_lowercase();
    Ok(host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .map_or_else(|| host.clone(), str::to_owned))
}

fn method(method: Method) -> http::Method {
    match method {
        Method::Get => http::Method::GET,
        Method::Head => http::Method::HEAD,
        Method::Post => http::Method::POST,
        Method::Put => http::Method::PUT,
        Method::Patch => http::Method::PATCH,
        Method::Delete => http::Method::DELETE,
    }
}

/// The plugin's headers, less [`DROPPED_REQUEST_HEADERS`]. A malformed one is an error.
fn request_headers(headers: &[(String, String)]) -> Result<HeaderMap, Error> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| Error::InvalidArgument(format!("header {name:?}: {error}")))?;
        if DROPPED_REQUEST_HEADERS.contains(&name.as_str()) {
            continue;
        }
        let value = HeaderValue::from_str(value)
            .map_err(|error| Error::InvalidArgument(format!("header {name}: {error}")))?;
        map.append(name, value);
    }
    Ok(map)
}

/// The response's headers, less [`DROPPED_RESPONSE_HEADERS`], their values read as UTF-8.
fn response_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| !DROPPED_RESPONSE_HEADERS.contains(&name.as_str()))
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// The import's error for fe-net's: a refused host is `denied`, as kiln's own check says.
fn net_error(url: &str, error: NetError) -> Error {
    match error {
        NetError::HostNotAllowed(_) => Error::Denied(format!("{url}: {error}")),
        NetError::Scheme => Error::InvalidArgument(format!("{url}: {error}")),
        _ => Error::Failed(format!("{url}: {error}")),
    }
}

/// `future`, or a timeout error after `duration`.
async fn timeout<T>(
    duration: Duration,
    url: &str,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    match select(pin!(future), async_io::Timer::after(duration)).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err(Error::Failed(format!("{url} timed out"))),
    }
}
