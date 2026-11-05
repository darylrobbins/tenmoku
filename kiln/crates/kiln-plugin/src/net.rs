// SPDX-License-Identifier: Apache-2.0

//! The host's network imports for plugins: `fetch`, which reaches only the hosts the plugin's
//! `net:<host>` grants name (or any, with `net:*`).
//!
//! The plugin has no socket: the host sends its request through a [`NetBackend`], and enforces
//! the grant here, before anything is sent and again on every redirect, so the policy is the
//! same whatever sends the bytes and is testable without one.
//!
//! - The URL is parsed as a browser does (WHATWG), and the check is on its host in normal
//!   form: lower case, its trailing dot removed, in its IDNA ASCII (punycode) form, and an IP
//!   address in its canonical text. It must equal a granted host; never a substring or suffix
//!   of one. An IP address is reached only by a grant of that address, or `net:*`; there is
//!   no grant for an IPv6 address short of `net:*`.
//! - A URL with a user name or password is refused. The scheme is `https`, or `http` to the
//!   loopback addresses only, for tests.
//! - The host follows redirects (301, 302, 303, 307 and 308), at most [`MAX_REDIRECTS`] of
//!   them, re-checking each `Location` against the grants. `https` never redirects to `http`.
//!   A redirect to another origin drops the request's credentials, and one that turns the
//!   request into a `GET` drops its body.
//! - The backend reports the host it connected to, which must equal the host checked.
//! - A response body over [`Limits::max_read_bytes`](crate::limits::Limits) is an error.

use std::fmt;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants, Target};
use url::{Host as UrlHost, Url};

use crate::bindings::kiln::extension::net::{Host, Method, Request, Response};
use crate::bindings::{Error, HostState};
use crate::limits::{Limits, MAX_READ_BYTES};

/// How many redirects one fetch follows; one more is an error.
pub const MAX_REDIRECTS: usize = 5;

/// How long a fetch may take, redirects and all, when the plugin names no timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The request headers that carry credentials, dropped on a redirect to another origin.
const CREDENTIAL_HEADERS: [&str; 3] = ["authorization", "cookie", "proxy-authorization"];

/// The request headers that describe its body, dropped with the body.
const BODY_HEADERS: [&str; 4] = [
    "content-encoding",
    "content-language",
    "content-location",
    "content-type",
];

/// One request the host sends for a plugin, after its URL passed the checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub method: Method,
    /// The URL in normal form: no user name, password or fragment, the host as checked.
    pub url: String,
    /// The host checked against the grants: lower case, no trailing dot, an IPv6 address
    /// without its brackets. The backend connects to this host and no other.
    pub host: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// What is left of the fetch's timeout.
    pub timeout: Duration,
    /// The most body bytes the host accepts: a backend may stop reading past it.
    pub max_body_bytes: usize,
}

/// What a backend received for an [`Outgoing`] request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The host the backend connected to, in [`Outgoing::host`]'s form.
    pub peer: String,
}

/// Sends a plugin's requests: one request and its response, redirects returned as they are.
pub trait NetBackend: Send {
    fn send(&mut self, request: &Outgoing) -> Result<Received, Error>;
}

/// The network slice of [`HostState`]: the plugin's grants and the backend that sends for it.
/// The default reaches no host.
pub struct State {
    grants: Grants,
    max_body_bytes: usize,
    backend: Option<Box<dyn NetBackend>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            grants: Grants::new(),
            max_body_bytes: MAX_READ_BYTES,
            backend: None,
        }
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("grants", &self.grants)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("backend", &self.backend.is_some())
            .finish()
    }
}

impl State {
    /// A slice that reaches the hosts in `grants`' `net:` capabilities through `backend`,
    /// reading at most `limits.max_read_bytes` of a response's body.
    pub fn new(grants: &Grants, limits: &Limits, backend: Box<dyn NetBackend>) -> Self {
        Self {
            grants: grants.clone(),
            max_body_bytes: limits.max_read_bytes,
            backend: Some(backend),
        }
    }

    /// Sends `request` if the grants reach its host, following redirects to granted hosts.
    pub fn fetch(&mut self, request: Request) -> Result<Response, Error> {
        let timeout = request
            .timeout_ms
            .map_or(DEFAULT_TIMEOUT, |ms| Duration::from_millis(ms.into()));
        let deadline = Instant::now() + timeout;
        let (mut url, host) = check(&self.grants, &request.url)?;
        let Some(backend) = &mut self.backend else {
            return Err(Error::Failed("the host has no network".into()));
        };
        let mut outgoing = Outgoing {
            method: request.method,
            url: url.to_string(),
            host,
            headers: request.headers,
            body: request.body,
            timeout,
            max_body_bytes: self.max_body_bytes,
        };
        let mut redirects = 0;
        loop {
            outgoing.timeout = deadline.saturating_duration_since(Instant::now());
            if outgoing.timeout.is_zero() {
                return Err(Error::Failed(format!("{} timed out", request.url)));
            }
            let received = backend.send(&outgoing)?;
            if received.peer != outgoing.host {
                return Err(Error::Denied(format!(
                    "{} connected to {}, not {}",
                    outgoing.url, received.peer, outgoing.host
                )));
            }
            if received.body.len() > self.max_body_bytes {
                return Err(Error::Failed(format!(
                    "{}'s response is over {} bytes",
                    outgoing.url, self.max_body_bytes
                )));
            }
            let Some(location) = redirect_location(&received)? else {
                return Ok(Response {
                    status: received.status,
                    headers: received.headers,
                    body: received.body,
                });
            };
            if redirects == MAX_REDIRECTS {
                return Err(Error::Failed(format!(
                    "{} redirected more than {MAX_REDIRECTS} times",
                    request.url
                )));
            }
            redirects += 1;
            let next = url
                .join(location)
                .map_err(|error| redirect_error(&url, location, &error.to_string()))?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(Error::Denied(format!(
                    "{url} redirected to {next}, which isn't https"
                )));
            }
            let (next, host) = check(&self.grants, next.as_str()).map_err(|error| match error {
                Error::InvalidArgument(why) => redirect_error(&url, location, &why),
                error => error,
            })?;
            if next.origin() != url.origin() {
                outgoing
                    .headers
                    .retain(|(name, _)| !is_one_of(name, &CREDENTIAL_HEADERS));
            }
            let to_get = match received.status {
                303 => outgoing.method != Method::Head,
                301 | 302 => outgoing.method == Method::Post,
                _ => false,
            };
            if to_get {
                outgoing.method = Method::Get;
                outgoing.body = None;
                outgoing
                    .headers
                    .retain(|(name, _)| !is_one_of(name, &BODY_HEADERS));
            }
            outgoing.url = next.to_string();
            outgoing.host = host;
            url = next;
        }
    }
}

impl Host for HostState {
    fn fetch(&mut self, request: Request) -> Result<Response, Error> {
        self.net.fetch(request)
    }
}

/// Parses `url` and checks it: the URL in normal form and its host, or why it is refused.
fn check(grants: &Grants, url: &str) -> Result<(Url, String), Error> {
    let mut parsed =
        Url::parse(url).map_err(|error| invalid(url, &format!("isn't a URL: {error}")))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid(url, "has a user name, which isn't sent"));
    }
    parsed.set_fragment(None);
    let (host, dotted) = match parsed.host() {
        Some(UrlHost::Domain(domain)) => {
            let host = domain.strip_suffix('.').unwrap_or(domain);
            if host.split('.').any(str::is_empty) {
                return Err(invalid(url, "has an empty label in its host"));
            }
            (host.to_owned(), host.len() != domain.len())
        }
        Some(UrlHost::Ipv4(address)) => (address.to_string(), false),
        Some(UrlHost::Ipv6(address)) => (address.to_string(), false),
        None => return Err(invalid(url, "has no host")),
    };
    if dotted {
        // The URL sent names the host checked.
        parsed
            .set_host(Some(&host))
            .map_err(|error| invalid(url, &error.to_string()))?;
    }
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback(&host) => {}
        _ => {
            return Err(invalid(
                url,
                "isn't https (http only to 127.0.0.1 or [::1])",
            ));
        }
    }
    if !grants.allows(&Capability::Net(Target::Named(host.clone()))) {
        return Err(Error::Denied(format!(
            "{url}: no net:{host} grant reaches its host"
        )));
    }
    Ok((parsed, host))
}

/// The `Location` a redirect response names, if `received` is one. A redirect with no
/// `Location` is returned as it is.
fn redirect_location(received: &Received) -> Result<Option<&str>, Error> {
    if !matches!(received.status, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    let mut locations = received
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("location"))
        .map(|(_, value)| value.as_str());
    let location = locations.next();
    if locations.next().is_some() {
        return Err(Error::Failed(format!(
            "a {} response has more than one Location",
            received.status
        )));
    }
    Ok(location)
}

/// The plugin's URL is malformed or not one the host sends.
fn invalid(url: &str, why: &str) -> Error {
    Error::InvalidArgument(format!("{url:?} {why}"))
}

/// The server's redirect is malformed: the plugin's request was fine.
fn redirect_error(from: &Url, location: &str, why: &str) -> Error {
    Error::Failed(format!("{from} redirected to {location:?}: {why}"))
}

/// The loopback addresses plain `http` may reach, for tests.
/// `host` is in canonical form, so each has one spelling.
fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1"
}

fn is_one_of(name: &str, names: &[&str]) -> bool {
    names.iter().any(|known| name.eq_ignore_ascii_case(known))
}
