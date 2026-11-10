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
//! - A fetch counts with the call's program runs against the [`PROGRAM_DEADLINE`](crate::limits::PROGRAM_DEADLINE) share (10 s),
//!   not against the 100 ms call deadline: its wall time moves that deadline back, and its
//!   timeout is capped at what is left of the share. Outside a call nothing is capped or charged.
//! - A response arriving after the fetch's timeout is a timeout error.
//! - A response body over [`Limits::max_read_bytes`](crate::limits::Limits) is an error.

use std::fmt;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants, Target};
use kiln_l10n::tr;
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
        self.fetch_within(request, None)
    }

    /// [`Self::fetch`], its timeout (the request's own, or [`DEFAULT_TIMEOUT`]) capped at
    /// `share`, what is left of the call's program share, if a call is running.
    pub fn fetch_within(
        &mut self,
        request: Request,
        share: Option<Duration>,
    ) -> Result<Response, Error> {
        let timeout = request
            .timeout_ms
            .map_or(DEFAULT_TIMEOUT, |ms| Duration::from_millis(ms.into()));
        let timeout = share.map_or(timeout, |share| share.min(timeout));
        let deadline = Instant::now() + timeout;
        let (mut url, host) = check(&self.grants, &request.url)?;
        let Some(backend) = &mut self.backend else {
            return Err(Error::Failed(tr!("kiln-plugin-net-none").to_string()));
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
                return Err(Error::Failed(
                    tr!("kiln-plugin-net-timed-out", url = &request.url).to_string(),
                ));
            }
            let received = backend.send(&outgoing)?;
            if Instant::now() > deadline {
                return Err(Error::Failed(
                    tr!("kiln-plugin-net-timed-out", url = &request.url).to_string(),
                ));
            }
            if received.peer != outgoing.host {
                return Err(Error::Denied(
                    tr!(
                        "kiln-plugin-net-wrong-peer",
                        url = &outgoing.url,
                        peer = &received.peer,
                        host = &outgoing.host
                    )
                    .to_string(),
                ));
            }
            if received.body.len() > self.max_body_bytes {
                return Err(Error::Failed(
                    tr!(
                        "kiln-plugin-net-too-large",
                        url = &outgoing.url,
                        limit = self.max_body_bytes
                    )
                    .to_string(),
                ));
            }
            let Some(location) = redirect_location(&received)? else {
                return Ok(Response {
                    status: received.status,
                    headers: received.headers,
                    body: received.body,
                });
            };
            if redirects == MAX_REDIRECTS {
                return Err(Error::Failed(
                    tr!(
                        "kiln-plugin-net-too-many-redirects",
                        url = &request.url,
                        limit = MAX_REDIRECTS
                    )
                    .to_string(),
                ));
            }
            redirects += 1;
            let next = url
                .join(location)
                .map_err(|error| redirect_error(&url, location, &error.to_string()))?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(Error::Denied(
                    tr!(
                        "kiln-plugin-net-redirect-not-https",
                        url = url.to_string(),
                        next = next.to_string()
                    )
                    .to_string(),
                ));
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
        // A fetch isn't charged to the call: its wall time moves the call's deadline back and
        // counts against the call's program share, which also caps its timeout.
        let started = Instant::now();
        let share = self.program_runs.remaining();
        let fetched = self.net.fetch_within(request, share);
        if share.is_some() {
            let charged = self.program_runs.charge(started.elapsed());
            self.deadline = self
                .deadline
                .and_then(|deadline| deadline.checked_add(charged));
        }
        fetched
    }
}

/// Parses `url` and checks it: the URL in normal form and its host, or why it is refused.
fn check(grants: &Grants, url: &str) -> Result<(Url, String), Error> {
    let mut parsed = Url::parse(url).map_err(|error| {
        invalid(
            tr!(
                "kiln-plugin-net-not-a-url",
                url = quoted(url),
                reason = error.to_string()
            )
            .to_string(),
        )
    })?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid(
            tr!("kiln-plugin-net-user-name", url = quoted(url)).to_string(),
        ));
    }
    parsed.set_fragment(None);
    let (host, dotted) = match parsed.host() {
        Some(UrlHost::Domain(domain)) => {
            let host = domain.strip_suffix('.').unwrap_or(domain);
            if host.split('.').any(str::is_empty) {
                return Err(invalid(
                    tr!("kiln-plugin-net-empty-label", url = quoted(url)).to_string(),
                ));
            }
            (host.to_owned(), host.len() != domain.len())
        }
        Some(UrlHost::Ipv4(address)) => (address.to_string(), false),
        Some(UrlHost::Ipv6(address)) => (address.to_string(), false),
        None => {
            return Err(invalid(
                tr!("kiln-plugin-net-no-host", url = quoted(url)).to_string(),
            ));
        }
    };
    if dotted {
        // The URL sent names the host checked.
        parsed.set_host(Some(&host)).map_err(|error| {
            invalid(
                tr!(
                    "kiln-plugin-net-bad-host",
                    url = quoted(url),
                    reason = error.to_string()
                )
                .to_string(),
            )
        })?;
    }
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback(&host) => {}
        _ => {
            return Err(invalid(
                tr!("kiln-plugin-net-not-https", url = quoted(url)).to_string(),
            ));
        }
    }
    if !grants.allows(&Capability::Net(Target::Named(host.clone()))) {
        return Err(Error::Denied(
            tr!(
                "kiln-plugin-net-not-granted",
                url = url,
                host = host.as_str()
            )
            .to_string(),
        ));
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
        return Err(Error::Failed(
            tr!("kiln-plugin-net-locations", status = received.status).to_string(),
        ));
    }
    Ok(location)
}

/// The plugin's URL is malformed or not one the host sends: `message` says
/// which URL, and why.
fn invalid(message: String) -> Error {
    Error::InvalidArgument(message)
}

/// `url`, quoted as written, as the messages about a refused URL name it.
fn quoted(url: &str) -> String {
    format!("{url:?}")
}

/// The server's redirect is malformed: the plugin's request was fine.
/// `why` is [`check`]'s message about the location.
fn redirect_error(from: &Url, location: &str, why: &str) -> Error {
    Error::Failed(
        tr!(
            "kiln-plugin-net-bad-redirect",
            url = from.to_string(),
            location = format!("{location:?}"),
            reason = why
        )
        .to_string(),
    )
}

/// The loopback addresses plain `http` may reach, for tests.
/// `host` is in canonical form, so each has one spelling.
fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1"
}

fn is_one_of(name: &str, names: &[&str]) -> bool {
    names.iter().any(|known| name.eq_ignore_ascii_case(known))
}
