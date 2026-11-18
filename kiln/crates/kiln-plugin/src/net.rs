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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants, Target};
use kiln_l10n::{tr, verbatim};
use url::{Host as UrlHost, Url};

use crate::bindings::kiln::extension::net::{Host, Method, Request, Response};
use crate::bindings::{Error, HostState};
use crate::jobs::{self, JobControl};
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

    /// [`Self::send`], given up as soon as it can be once `cancel` is cancelled, which fails it
    /// with `failed("cancelled")`. The host cancels a job's send when the job is being stopped
    /// or the fetch's deadline passes, and doesn't wait for it; the send's thread, and the
    /// request it holds, last until this returns. The default doesn't watch `cancel`.
    fn send_cancellable(
        &mut self,
        request: &Outgoing,
        cancel: &SendCancel,
    ) -> Result<Received, Error> {
        let _ = cancel;
        self.send(request)
    }

    /// Whether [`Self::send_cancellable`] gives a send up once it is cancelled: only such a
    /// backend fetches for a background job. The default is false.
    fn cancellable(&self) -> bool {
        false
    }
}

/// Whether the host has called a send off: see [`NetBackend::send_cancellable`]. Clones share
/// one flag.
#[derive(Clone, Debug, Default)]
pub struct SendCancel(Arc<AtomicBool>);

impl SendCancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The network slice of [`HostState`]: the plugin's grants and the backend that sends for it.
/// The default reaches no host.
pub struct State {
    grants: Grants,
    max_body_bytes: usize,
    backend: Option<Box<dyn NetBackend>>,
    /// In a job, a send that timed out before its backend returned: the backend comes back
    /// through it, to the next fetch.
    pending: Option<Receiver<Sent>>,
}

/// What a job's send thread hands back: the backend, and, unless the send was called off by
/// then, what it received and the request, counted as in flight until they are dropped.
struct Sent {
    backend: Box<dyn NetBackend>,
    reply: Option<(Result<Received, Error>, Outgoing, JobSend)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            grants: Grants::new(),
            max_body_bytes: MAX_READ_BYTES,
            backend: None,
            pending: None,
        }
    }
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("grants", &self.grants)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("backend", &self.backend.is_some())
            .field("pending", &self.pending.is_some())
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
            pending: None,
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
        self.fetch_watching(request, share, None)
    }

    /// [`Self::fetch_within`], in a job whose `stop` it watches: once the job is being stopped,
    /// it fails as cancelled, whatever the backend is still waiting for.
    fn fetch_watching(
        &mut self,
        request: Request,
        share: Option<Duration>,
        stop: Option<&JobControl>,
    ) -> Result<Response, Error> {
        if stop.is_some_and(JobControl::is_cancelled) {
            return Err(Error::Failed(jobs::CANCELLED.to_owned()));
        }
        let timeout = request
            .timeout_ms
            .map_or(DEFAULT_TIMEOUT, |ms| Duration::from_millis(ms.into()));
        let timeout = share.map_or(timeout, |share| share.min(timeout));
        let deadline = Instant::now() + timeout;
        let (mut url, host) = check(&self.grants, &request.url)?;
        if self.backend.is_none() && self.pending.is_none() {
            return Err(no_backend());
        }
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
            let timed_out =
                || Error::Failed(tr!("kiln-plugin-net-timed-out", url = &request.url).to_string());
            let received;
            (received, outgoing) = self.send(outgoing, deadline, timed_out, stop)?;
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

/// What sends for a plugin with no backend answers.
fn no_backend() -> Error {
    Error::Failed(tr!("kiln-plugin-net-none").to_string())
}

/// How many of jobs' sends may be in flight at once, those of stopped jobs still finishing
/// among them.
pub const MAX_JOB_SENDS: usize = 16;

/// The most bytes of request (URL, headers and body) jobs' sends in flight hold together.
pub const MAX_JOB_SEND_BYTES: usize = 32 << 20;

/// `failed`: a job's fetch through a backend that can't give a send up
/// ([`NetBackend::cancellable`]).
pub const NOT_CANCELLABLE: &str = verbatim!("this backend can't fetch for a background job");

/// `failed`: past [`MAX_JOB_SENDS`] or [`MAX_JOB_SEND_BYTES`].
pub const TOO_MANY_SENDS: &str = verbatim!("too many fetches of jobs in flight");

/// Jobs' sends in flight, and the request bytes they hold.
static JOB_SENDS: Mutex<(usize, usize)> = Mutex::new((0, 0));

/// How many of jobs' sends are in flight, and the request bytes they hold: a send stays in
/// flight, on its own thread, until its backend returns, after its job has stopped waiting too.
pub fn job_sends() -> (usize, usize) {
    *JOB_SENDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One of jobs' sends in flight, until dropped.
struct JobSend(usize);

impl JobSend {
    /// Counts a send of `bytes` of request, if it fits.
    fn start(bytes: usize) -> Option<Self> {
        let mut sends = JOB_SENDS.lock().unwrap_or_else(PoisonError::into_inner);
        if sends.0 >= MAX_JOB_SENDS || sends.1.saturating_add(bytes) > MAX_JOB_SEND_BYTES {
            return None;
        }
        sends.0 += 1;
        sends.1 += bytes;
        Some(Self(bytes))
    }
}

impl Drop for JobSend {
    fn drop(&mut self) {
        let mut sends = JOB_SENDS.lock().unwrap_or_else(PoisonError::into_inner);
        sends.0 -= 1;
        sends.1 -= self.0;
    }
}

/// The most headers a job's request carries.
pub const MAX_JOB_HEADERS: usize = 256;

/// `invalid-argument`: past [`MAX_JOB_HEADERS`].
pub const TOO_MANY_HEADERS: &str = verbatim!("too many headers");

/// The bytes `outgoing` holds, its containers' spare room among them.
fn request_bytes(outgoing: &Outgoing) -> usize {
    let headers: usize = (outgoing.headers.iter())
        .map(|(name, value)| name.capacity() + value.capacity())
        .sum();
    std::mem::size_of::<Outgoing>()
        + outgoing.url.capacity()
        + outgoing.host.capacity()
        + outgoing.headers.capacity() * std::mem::size_of::<(String, String)>()
        + headers
        + outgoing.body.as_ref().map_or(0, Vec::capacity)
}

/// How waiting on a job's send ended.
enum Waited {
    Sent(Box<Sent>),
    /// The job is being stopped; the send goes on.
    Stopped(Receiver<Sent>),
    /// Its deadline passed; the send goes on.
    TimedOut(Receiver<Sent>),
    /// Its thread ended without handing the backend back.
    Gone,
}

/// Waits on `sent` until it hands the backend back, `stop` is cancelled or `deadline` passes.
fn wait(sent: Receiver<Sent>, deadline: Instant, stop: &JobControl) -> Waited {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match sent.recv_timeout(left.min(jobs::STOP_POLL)) {
            Ok(back) => return Waited::Sent(Box::new(back)),
            Err(RecvTimeoutError::Timeout) if stop.is_cancelled() => return Waited::Stopped(sent),
            Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                return Waited::TimedOut(sent);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Waited::Gone,
        }
    }
}

impl State {
    /// Sends `outgoing` through the backend, and hands it back with what was received. In a job
    /// (`stop`), the backend sends on a thread of its own while this one waits until the job is
    /// being stopped, which fails it as cancelled, or `deadline`, which is `timed_out`. Either
    /// way the send is called off ([`NetBackend::send_cancellable`]) and left to end on its
    /// thread, holding the backend, which the next fetch waits for, within its own deadline,
    /// before it sends. Those threads, and the requests they
    /// hold, are capped ([`MAX_JOB_SENDS`], [`MAX_JOB_SEND_BYTES`]) whatever the jobs' slots,
    /// and a job's request carries at most [`MAX_JOB_HEADERS`].
    fn send(
        &mut self,
        outgoing: Outgoing,
        deadline: Instant,
        timed_out: impl Fn() -> Error,
        stop: Option<&JobControl>,
    ) -> Result<(Received, Outgoing), Error> {
        let Some(stop) = stop else {
            return match &mut self.backend {
                Some(backend) => Ok((backend.send(&outgoing)?, outgoing)),
                None => Err(no_backend()),
            };
        };
        let cancelled = || Err(Error::Failed(jobs::CANCELLED.to_owned()));
        if stop.is_cancelled() {
            return cancelled();
        }
        if outgoing.headers.len() > MAX_JOB_HEADERS {
            return Err(Error::InvalidArgument(TOO_MANY_HEADERS.to_owned()));
        }
        // A send that timed out before may still hold the backend.
        if self.backend.is_none()
            && let Some(pending) = self.pending.take()
        {
            match wait(pending, deadline, stop) {
                Waited::Sent(back) => self.backend = Some(back.backend),
                Waited::Stopped(pending) => {
                    self.pending = Some(pending);
                    return cancelled();
                }
                Waited::TimedOut(pending) => {
                    self.pending = Some(pending);
                    return Err(timed_out());
                }
                Waited::Gone => return Err(no_backend()),
            }
        }
        if !self
            .backend
            .as_ref()
            .is_some_and(|backend| backend.cancellable())
        {
            return Err(Error::Failed(NOT_CANCELLABLE.to_owned()));
        }
        let Some(in_flight) = JobSend::start(request_bytes(&outgoing)) else {
            return Err(Error::Failed(TOO_MANY_SENDS.to_owned()));
        };
        let Some(mut backend) = self.backend.take() else {
            return Err(no_backend());
        };
        let (sender, sent) = mpsc::channel();
        let cancel = SendCancel::default();
        let sending = cancel.clone();
        thread::Builder::new()
            .spawn(move || {
                let received = backend.send_cancellable(&outgoing, &sending);
                // Called off, nobody reads what came: it goes now, with its count.
                let reply = (!sending.is_cancelled()).then_some((received, outgoing, in_flight));
                drop(sender.send(Sent { backend, reply }));
            })
            .map_err(|error| Error::Failed(error.to_string()))?;
        // A send this stops waiting for is called off, so a backend that watches for that
        // gives its thread and request back at once.
        let waited = wait(sent, deadline, stop);
        if !matches!(waited, Waited::Sent(_)) {
            cancel.cancel();
        }
        match waited {
            Waited::Sent(back) => {
                let Sent { backend, reply } = *back;
                self.backend = Some(backend);
                match reply {
                    Some((received, outgoing, _in_flight)) => Ok((received?, outgoing)),
                    None => cancelled(),
                }
            }
            Waited::Stopped(sent) => {
                self.pending = Some(sent);
                cancelled()
            }
            Waited::TimedOut(sent) => {
                self.pending = Some(sent);
                Err(timed_out())
            }
            Waited::Gone => Err(no_backend()),
        }
    }
}

impl Host for HostState {
    fn fetch(&mut self, request: Request) -> Result<Response, Error> {
        // A fetch isn't charged to the call: its wall time moves the call's deadline back and
        // counts against the call's program share, which also caps its timeout.
        let started = Instant::now();
        let share = self.program_runs.remaining();
        let fetched = self.net.fetch_watching(request, share, self.jobs.control());
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use kiln_command::capability::Capability;

    use super::*;

    /// Answers each request `200` after the next of its delays.
    struct Slow(VecDeque<Duration>);

    impl NetBackend for Slow {
        fn cancellable(&self) -> bool {
            true
        }

        fn send(&mut self, request: &Outgoing) -> Result<Received, Error> {
            thread::sleep(self.0.pop_front().unwrap_or_default());
            Ok(Received {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
                peer: request.host.clone(),
            })
        }
    }

    fn slow(delays: &[u64]) -> State {
        let grants: Grants = [Capability::parse("net:example.com").unwrap()]
            .into_iter()
            .collect();
        let delays = delays.iter().map(|&ms| Duration::from_millis(ms));
        State::new(
            &grants,
            &Limits::default(),
            Box::new(Slow(delays.collect())),
        )
    }

    fn get(timeout_ms: u32, headers: usize) -> Request {
        Request {
            method: Method::Get,
            url: "https://example.com/".to_owned(),
            headers: vec![("cookie".to_owned(), String::new()); headers],
            body: None,
            timeout_ms: Some(timeout_ms),
        }
    }

    #[test]
    fn a_job_fetch_after_one_that_timed_out_gets_the_backend_back() {
        let mut state = slow(&[300, 0]);
        let job = JobControl::default();
        // The backend answers after 300 ms: past this fetch's 50.
        let first = state.fetch_watching(get(50, 0), None, Some(&job));
        assert!(matches!(first, Err(Error::Failed(_))), "{first:?}");
        // The next waits for the backend, within its own timeout, and sends.
        let second = state.fetch_watching(get(5_000, 0), None, Some(&job));
        assert_eq!(second.map(|response| response.status), Ok(200));
        assert!(no_sends_in_flight(), "{:?}", job_sends());
    }

    /// Waits up to 10 s for no job's send to be in flight.
    fn no_sends_in_flight() -> bool {
        let until = Instant::now() + Duration::from_secs(10);
        while job_sends() != (0, 0) && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        job_sends() == (0, 0)
    }

    #[test]
    fn a_timed_out_send_keeps_nothing_but_the_backend() {
        let mut state = slow(&[300]);
        let job = JobControl::default();
        let first = state.fetch_watching(get(50, 0), None, Some(&job));
        assert!(matches!(first, Err(Error::Failed(_))), "{first:?}");
        // The job goes on without fetching again: once the backend returns, nothing of the
        // request or its answer is held, only the backend, waiting for the next fetch.
        assert!(no_sends_in_flight(), "{:?}", job_sends());
        assert!(state.pending.is_some());
    }

    /// A backend that can't give a send up.
    struct Plain;

    impl NetBackend for Plain {
        fn send(&mut self, request: &Outgoing) -> Result<Received, Error> {
            Ok(Received {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
                peer: request.host.clone(),
            })
        }
    }

    #[test]
    fn a_backend_that_cant_give_a_send_up_doesnt_fetch_for_a_job() {
        let grants: Grants = [Capability::parse("net:example.com").unwrap()]
            .into_iter()
            .collect();
        let mut state = State::new(&grants, &Limits::default(), Box::new(Plain));
        let job = JobControl::default();
        let refused = state.fetch_watching(get(5_000, 0), None, Some(&job));
        assert_eq!(refused, Err(Error::Failed(NOT_CANCELLABLE.to_owned())));
        // An interactive call's fetch is waited for, and goes through it.
        let fetched = state.fetch_watching(get(5_000, 0), None, None);
        assert_eq!(fetched.map(|response| response.status), Ok(200));
    }

    #[test]
    fn a_job_request_with_too_many_headers_is_refused() {
        let mut state = slow(&[]);
        let job = JobControl::default();
        let refused = state.fetch_watching(get(5_000, MAX_JOB_HEADERS + 1), None, Some(&job));
        assert_eq!(
            refused,
            Err(Error::InvalidArgument(TOO_MANY_HEADERS.to_owned()))
        );
        let sent = state.fetch_watching(get(5_000, MAX_JOB_HEADERS), None, Some(&job));
        assert_eq!(sent.map(|response| response.status), Ok(200));
    }

    #[test]
    fn a_request_is_counted_with_its_containers_and_spare_room() {
        let mut headers = Vec::with_capacity(1000);
        headers.extend((0..10).map(|_| ("cookie".to_owned(), String::new())));
        let outgoing = Outgoing {
            method: Method::Get,
            url: String::with_capacity(4096),
            host: "example.com".to_owned(),
            headers,
            body: Some(Vec::with_capacity(1 << 20)),
            timeout: Duration::ZERO,
            max_body_bytes: 0,
        };
        let bytes = request_bytes(&outgoing);
        let containers = 1000 * std::mem::size_of::<(String, String)>();
        assert!(bytes >= 4096 + containers + (1 << 20) + 10 * 6, "{bytes}");
    }
}
