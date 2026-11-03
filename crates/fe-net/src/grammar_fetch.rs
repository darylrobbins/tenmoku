//! [`grammar_fetch`]: downloads a first-party grammar and returns its bytes
//! only if they hash to what the app recorded (the decision record:
//! `docs/decisions/c1pt00h80c-grammar-fetch.md`).
//!
//! [`Client`] doesn't follow redirects, and a GitHub release asset answers
//! with one to its storage host, so this follows a few for itself. Each hop is
//! an ordinary [`Client::request`], so the client's grants decide it: a hop to
//! a host they don't name is refused before a DNS lookup or a socket.

use futures::StreamExt as _;
use http::header::{CONTENT_LENGTH, LOCATION, USER_AGENT};
use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use sha2::{Digest as _, Sha256};

use crate::stream::ReadLimit;
use crate::{Client, NetError, Response};

/// How many redirects one fetch follows.
pub const MAX_REDIRECTS: usize = 3;

/// Why a grammar wasn't returned.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GrammarFetchError {
    /// No hash is recorded for it: it isn't released yet. Nothing was sent.
    #[error("this grammar isn't released yet")]
    NotReleased,
    /// The recorded hash isn't 64 lowercase hex digits. Nothing was sent.
    #[error("the recorded hash {0:?} isn't a SHA-256")]
    InvalidHash(String),
    /// The bytes hash to something other than what's recorded.
    #[error("the download's SHA-256 is {actual}, not the recorded {expected}")]
    HashMismatch { expected: String, actual: String },
    /// The body is longer than the caller allows.
    #[error("the download is over the limit of {max_bytes} bytes")]
    TooLarge { max_bytes: u64 },
    /// A redirect, or the URL itself, names a host outside the grants. Nothing
    /// was sent to it.
    #[error("{0} is not a host this download may reach")]
    HostNotAllowed(String),
    /// More than [`MAX_REDIRECTS`] redirects.
    #[error("the download redirected more than {MAX_REDIRECTS} times")]
    TooManyRedirects,
    /// A redirect without a `Location` this follows.
    #[error("the download redirected to {0:?}, which can't be followed")]
    BadRedirect(String),
    /// A final status other than 200.
    #[error("the server answered {0}")]
    Status(u16),
    /// The request failed, or its body stopped.
    #[error(transparent)]
    Net(NetError),
}

impl From<NetError> for GrammarFetchError {
    fn from(error: NetError) -> GrammarFetchError {
        match error {
            NetError::HostNotAllowed(host) => GrammarFetchError::HostNotAllowed(host),
            error => GrammarFetchError::Net(error),
        }
    }
}

/// `bytes`' SHA-256, in lowercase hex, as recorded in a pack manifest.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

fn hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The body at `url`, following up to [`MAX_REDIRECTS`] redirects, each only
/// to a host `client`'s grants name, once it's whole and its SHA-256 is
/// `expected_sha256`.
///
/// An empty `expected_sha256` is [`GrammarFetchError::NotReleased`], without a
/// request. A `Content-Length` over `max_bytes` is
/// [`GrammarFetchError::TooLarge`] before the body is read; so is the body's
/// reaching `max_bytes + 1`, the most of it ever read, at which point the
/// connection closes.
///
/// Nothing bounds how long it takes beyond the client's idle timeout: dropping
/// the future stops it and closes the connection.
pub async fn grammar_fetch(
    client: &Client,
    url: &str,
    expected_sha256: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, GrammarFetchError> {
    let limit = ReadLimit::new(max_bytes.saturating_add(1));
    fetch_limited(client, url, expected_sha256, max_bytes, &limit).await
}

async fn fetch_limited(
    client: &Client,
    url: &str,
    expected_sha256: &str,
    max_bytes: u64,
    limit: &ReadLimit,
) -> Result<Vec<u8>, GrammarFetchError> {
    if expected_sha256.is_empty() {
        return Err(GrammarFetchError::NotReleased);
    }
    if !is_sha256(expected_sha256) {
        return Err(GrammarFetchError::InvalidHash(expected_sha256.to_owned()));
    }
    let response = follow(client, url, limit).await?;
    if response.status != StatusCode::OK {
        return Err(GrammarFetchError::Status(response.status.as_u16()));
    }
    let announced = response
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok()?.parse::<u64>().ok());
    if announced.is_some_and(|length| length > max_bytes) {
        return Err(GrammarFetchError::TooLarge { max_bytes });
    }
    let mut body = response.body;
    let mut bytes = Vec::new();
    let mut hasher = Sha256::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        let received = (bytes.len() + chunk.len()) as u64;
        if received > max_bytes {
            return Err(GrammarFetchError::TooLarge { max_bytes });
        }
        hasher.update(&chunk);
        bytes.extend_from_slice(&chunk);
        limit.set((max_bytes - received).saturating_add(1));
    }
    let actual = hex(hasher.finalize().as_slice());
    if actual != expected_sha256 {
        return Err(GrammarFetchError::HashMismatch {
            expected: expected_sha256.to_owned(),
            actual,
        });
    }
    Ok(bytes)
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The response at the end of `url`'s redirects, its body unread.
async fn follow(
    client: &Client,
    url: &str,
    limit: &ReadLimit,
) -> Result<Response, GrammarFetchError> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("Tenmoku"));
    let mut url = url.to_owned();
    let mut redirects = 0;
    loop {
        let response = client
            .send(
                Method::GET,
                &url,
                headers.clone(),
                Vec::new(),
                Some(limit.clone()),
            )
            .await?;
        if !is_redirect(response.status) {
            return Ok(response);
        }
        if redirects == MAX_REDIRECTS {
            return Err(GrammarFetchError::TooManyRedirects);
        }
        redirects += 1;
        url = redirect_target(&url, response.headers.get(LOCATION))?;
    }
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

/// The URL a redirect from `from` names: an absolute URL, or one relative to
/// the scheme or the host. A path relative to `from`'s own isn't followed, as
/// GitHub sends none and refusing is simpler than resolving one.
fn redirect_target(
    from: &str,
    location: Option<&HeaderValue>,
) -> Result<String, GrammarFetchError> {
    let location = location.ok_or_else(|| GrammarFetchError::BadRedirect(String::new()))?;
    let bad =
        || GrammarFetchError::BadRedirect(String::from_utf8_lossy(location.as_bytes()).into());
    let location = location.to_str().map_err(|_| bad())?;
    let from: Uri = from.parse().map_err(|_| bad())?;
    let scheme = from.scheme_str().ok_or_else(bad)?;
    if location.starts_with("//") {
        return Ok(format!("{scheme}:{location}"));
    }
    if location.starts_with('/') {
        let authority = from.authority().ok_or_else(bad)?;
        return Ok(format!("{scheme}://{authority}{location}"));
    }
    let target: Uri = location.parse().map_err(|_| bad())?;
    if target.scheme().is_none() || target.authority().is_none() {
        return Err(bad());
    }
    Ok(location.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    use futures::executor::block_on;
    use kiln_command::{Capability, Grants};

    use super::*;

    const MAX: u64 = 1000;

    /// Fetches from a server that sends `head`, `framing`, then bytes without
    /// end, with a limit of [`MAX`]; returns how many bytes the connection read
    /// past `head` and `framing`.
    fn body_bytes_read(head: &'static str, framing: &'static str) -> u64 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(framing.as_bytes());
            // Far more than the limit, and than any buffer on the way.
            let body = vec![b'x'; 1 << 20];
            while stream.write_all(&body).is_ok() {}
        });
        let grants: Grants = [Capability::parse("net:127.0.0.1").unwrap()]
            .into_iter()
            .collect();
        let client = Client::new(&grants);
        let limit = ReadLimit::new(MAX + 1);
        let fetched = block_on(fetch_limited(
            &client,
            &format!("http://{addr}/x"),
            &sha256_hex(b""),
            MAX,
            &limit,
        ));
        assert_eq!(fetched, Err(GrammarFetchError::TooLarge { max_bytes: MAX }));
        limit.total_read() - (head.len() + framing.len()) as u64
    }

    /// One chunk far bigger than the budget: it stops at the budget, not at
    /// the chunk's end or hyper's buffer's.
    #[test]
    fn a_large_chunk_is_read_only_to_the_limit() {
        let read = body_bytes_read(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            "40000000\r\n",
        );
        assert_eq!(read, MAX + 1);
    }

    /// A body ended only by the connection closing.
    #[test]
    fn an_unframed_body_is_read_only_to_the_limit() {
        let read = body_bytes_read("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", "");
        assert_eq!(read, MAX + 1);
    }

    /// A limit bigger than memory can hold doesn't overflow.
    #[test]
    fn the_largest_limit_reads_normally() {
        assert_eq!(ReadLimit::new(u64::MAX).cap(8192), 8192);
        assert_eq!(ReadLimit::new(5).cap(8192), 5);
        assert_eq!(
            ReadLimit::new(0).cap(8192),
            1,
            "0 bytes would read as the end"
        );
    }
}
