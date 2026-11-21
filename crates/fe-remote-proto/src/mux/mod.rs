//! The multiplexer (D § 2.2, § 2.4, § 2.5): one link carrying requests and
//! streams on many channels, each held to its own credit, with a strict
//! handshake and a heartbeat.
//!
//! Each link gets three threads: a reader that routes frames, a writer that
//! drains the send queue, and a timer for the heartbeat. No thread is made
//! per request. Handles ([`Stream`], [`Mux::accept`]) block, or poll with a
//! `Waker`, so a thread pool and an async executor can both drive them
//! without either side needing a particular runtime.

mod state;
mod stream;

pub use stream::Stream;

use crate::clock::Clock;
use crate::frame::{self, check_body};
use crate::heartbeat::{HEARTBEAT, Heartbeat, MISSED_PONGS, Rtt};
use crate::link::Link;
use crate::window::{DeliveryRate, INITIAL_WINDOW, MIN_WINDOW};
use crate::{
    BUILD, FrameError, HEADER, Header, Hello, Kind, Location, PROTOCOL, Refused, Resume,
    SERVER_ORIGINATED, SessionToken, Status, StreamResume, Welcome, chan,
};
use serde::{Deserialize, Serialize};
use state::{Holder, Out, Role, RouteKind, Shared};
use std::future::poll_fn;
use std::io::{self, BufWriter, Read, Write};
use std::process::Child;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Small frames (credits, pongs, short replies) are coalesced up to this.
/// A chunk is larger, so it goes straight to the link and the writer never
/// holds more than the frame it's writing ahead of an urgent one.
const WRITE_BUFFER: usize = 16 * 1024;

#[derive(Clone)]
pub struct Config {
    /// What this side says it is, and requires of the peer (§ 2.5).
    pub protocol: u32,
    pub build: String,
    /// The credit this side grants each channel (§ 2.4).
    pub window: u32,
    /// How often this side pings; `None` for never.
    pub heartbeat: Option<Duration>,
    pub clock: Clock,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            protocol: PROTOCOL,
            build: BUILD.to_string(),
            window: INITIAL_WINDOW,
            heartbeat: Some(HEARTBEAT),
            clock: Clock::System,
        }
    }
}

/// What a server's accept callback gives a `Hello` it takes.
pub struct Accept {
    pub session: SessionToken,
    pub streams: Vec<StreamResume>,
}

/// What a [`Stream`] receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A `Data` body, as it came; [`Stream::decode`] reads a typed one.
    Data(Vec<u8>),
    /// The last event: the peer's `End`, or `LinkLost`.
    End(Status),
}

/// A request from the peer.
pub struct Incoming {
    pub header: Header,
    /// The request's body, the channel's op type in postcard.
    pub op: Vec<u8>,
    /// Where the reply goes.
    pub stream: Stream,
}

impl Incoming {
    pub fn op<'a, T: Deserialize<'a>>(&'a self) -> Result<T, FrameError> {
        frame::body(&self.header, &self.op)
    }
}

/// Why the link is gone.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Lost {
    #[error("the peer closed the link")]
    Eof,
    #[error("{MISSED_PONGS} pings in a row went unanswered")]
    MissedPongs,
    #[error("the link failed: {0}")]
    Io(String),
    #[error("the peer broke the protocol: {0}")]
    Protocol(String),
    #[error("this side closed the link")]
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MuxError {
    #[error("the link is lost: {0}")]
    LinkLost(Lost),
    #[error("the peer cancelled it")]
    Cancelled,
    #[error("it has ended")]
    Ended,
    #[error("a request is ended by the side that answers it")]
    OwnRequest,
    #[error("no credit on channel {0} yet")]
    Full(u16),
    #[error("a data body of {0} bytes is over {max}", max = crate::window::MAX_DATA)]
    TooLong(usize),
    #[error("channel {0} isn't a stream channel")]
    NotStream(u16),
    #[error("channel {0} already has a stream")]
    Taken(u16),
    #[error("channel {0} has carried a stream; each stream takes a fresh channel")]
    Spent(u16),
    #[error(transparent)]
    Frame(#[from] FrameError),
}

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    /// The server refused this client, or this server refused the client;
    /// with the reason the `Refused` frame carried.
    #[error("refused at the handshake: {0}")]
    Refused(String),
    /// The server took the `Hello` but answered as another protocol or
    /// build, so the client closed the link (§ 2.5: both peers check).
    #[error(
        "server answered protocol {protocol} build {build:?}, client is protocol {ours_protocol} build {ours_build:?}"
    )]
    Mismatch {
        protocol: u32,
        build: String,
        ours_protocol: u32,
        ours_build: String,
    },
    #[error("a window of {0} bytes is under the {MIN_WINDOW}-byte minimum")]
    Window(u32),
    #[error("the handshake got {0:?}")]
    Unexpected(Kind),
    #[error("the link closed during the handshake")]
    Closed,
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<HandshakeError> for io::Error {
    fn from(e: HandshakeError) -> io::Error {
        let kind = match &e {
            HandshakeError::Io(e) => return io::Error::new(e.kind(), e.to_string()),
            HandshakeError::Refused(_) => io::ErrorKind::ConnectionRefused,
            HandshakeError::Closed => io::ErrorKind::UnexpectedEof,
            HandshakeError::Window(_) => io::ErrorKind::InvalidInput,
            HandshakeError::Mismatch { .. }
            | HandshakeError::Unexpected(_)
            | HandshakeError::Frame(_) => io::ErrorKind::InvalidData,
        };
        io::Error::new(kind, e)
    }
}

/// One link's multiplexer. Dropping it (or [`Mux::close`]) hangs up: the
/// queue drains, the link closes, and every handle still open then sees
/// the link lost.
pub struct Mux {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

fn write_frame(w: &mut dyn Write, frame: &[u8]) -> io::Result<()> {
    w.write_all(frame)?;
    w.flush()
}

fn read_handshake(r: &mut dyn Read) -> Result<(Header, Vec<u8>), HandshakeError> {
    let (h, b) = frame::read_frame(&mut &mut *r)?.ok_or(HandshakeError::Closed)?;
    check_body(&h, &b)?;
    Ok((h, b))
}

fn client_handshake(
    r: &mut dyn Read,
    w: &mut dyn Write,
    config: &Config,
    location: Location,
    resume: Option<Resume>,
) -> Result<Welcome, HandshakeError> {
    let hello = Hello {
        protocol: config.protocol,
        build: config.build.clone(),
        window: config.window,
        resume,
        location,
    };
    write_frame(w, &frame::encode(Kind::Hello, chan::CONTROL, 0, &hello)?)?;
    let (h, b) = read_handshake(r)?;
    let welcome: Welcome = match h.kind {
        Kind::Welcome => frame::body(&h, &b)?,
        Kind::Refused => {
            return Err(HandshakeError::Refused(
                frame::body::<Refused>(&h, &b)?.reason,
            ));
        }
        k => return Err(HandshakeError::Unexpected(k)),
    };
    if welcome.protocol != config.protocol || welcome.build != config.build {
        return Err(HandshakeError::Mismatch {
            protocol: welcome.protocol,
            build: welcome.build,
            ours_protocol: config.protocol,
            ours_build: config.build.clone(),
        });
    }
    if welcome.window < MIN_WINDOW {
        return Err(HandshakeError::Window(welcome.window));
    }
    Ok(welcome)
}

fn server_handshake(
    r: &mut dyn Read,
    w: &mut dyn Write,
    config: &Config,
    accept: impl FnOnce(&Hello) -> Result<Accept, String>,
) -> Result<Hello, HandshakeError> {
    let (h, b) = read_handshake(r)?;
    if h.kind != Kind::Hello {
        return Err(HandshakeError::Unexpected(h.kind));
    }
    let hello: Hello = frame::body(&h, &b)?;
    let mut refuse = |reason: String| {
        // The refusal is a courtesy: the link closes either way.
        if let Ok(f) = frame::encode(
            Kind::Refused,
            chan::CONTROL,
            0,
            &Refused {
                reason: reason.clone(),
            },
        ) {
            let _ = write_frame(w, &f);
        }
        HandshakeError::Refused(reason)
    };
    if hello.protocol != config.protocol || hello.build != config.build {
        return Err(refuse(format!(
            "client protocol {} build {:?}, server protocol {} build {:?}",
            hello.protocol, hello.build, config.protocol, config.build
        )));
    }
    if hello.window < MIN_WINDOW {
        return Err(refuse(format!(
            "the client's window of {} bytes is under the {MIN_WINDOW}-byte minimum",
            hello.window
        )));
    }
    let accepted = accept(&hello).map_err(&mut refuse)?;
    let welcome = Welcome {
        protocol: config.protocol,
        build: config.build.clone(),
        window: config.window,
        session: accepted.session,
        streams: accepted.streams,
    };
    write_frame(
        w,
        &frame::encode(Kind::Welcome, chan::CONTROL, 0, &welcome)?,
    )?;
    Ok(hello)
}

/// Kills and reaps a child whose handshake failed.
fn abandon(child: Option<Child>) {
    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
}

impl Mux {
    /// Runs the client's half of the handshake: sends `Hello`, and takes
    /// only a `Welcome` with this side's protocol and build and a window
    /// of at least [`MIN_WINDOW`].
    pub fn client(
        link: Link,
        config: Config,
        location: Location,
        resume: Option<Resume>,
    ) -> Result<(Mux, Welcome), HandshakeError> {
        let Link {
            mut reader,
            mut writer,
            child,
        } = link;
        if config.window < MIN_WINDOW {
            abandon(child);
            return Err(HandshakeError::Window(config.window));
        }
        match client_handshake(&mut *reader, &mut *writer, &config, location, resume) {
            Ok(welcome) => {
                let mux = Mux::start(Role::Client, reader, writer, child, config, welcome.window)?;
                Ok((mux, welcome))
            }
            Err(e) => {
                drop(writer);
                abandon(child);
                Err(e)
            }
        }
    }

    /// Runs the server's half: refuses a `Hello` of another protocol or
    /// build, or with a window under [`MIN_WINDOW`], with the reason; then
    /// asks `accept` for the session, which may refuse it too.
    pub fn server(
        link: Link,
        config: Config,
        accept: impl FnOnce(&Hello) -> Result<Accept, String>,
    ) -> Result<(Mux, Hello), HandshakeError> {
        let Link {
            mut reader,
            mut writer,
            child,
        } = link;
        if config.window < MIN_WINDOW {
            abandon(child);
            return Err(HandshakeError::Window(config.window));
        }
        match server_handshake(&mut *reader, &mut *writer, &config, accept) {
            Ok(hello) => {
                let mux = Mux::start(Role::Server, reader, writer, child, config, hello.window)?;
                Ok((mux, hello))
            }
            Err(e) => {
                drop(writer);
                abandon(child);
                Err(e)
            }
        }
    }

    fn start(
        role: Role,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        child: Option<Child>,
        config: Config,
        peer_window: u32,
    ) -> io::Result<Mux> {
        let heartbeat = config
            .heartbeat
            .map(|every| Heartbeat::new(config.clock.now(), every, role.origin()));
        let beats = heartbeat.is_some();
        let shared = Arc::new(Shared::new(
            role,
            config.clock,
            config.window,
            peer_window,
            heartbeat,
            child,
        ));
        let mut threads = Vec::new();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            #[cfg(feature = "test-support")]
            crate::testing::injected_spawn_failure()?;
            thread::Builder::new()
                .name(format!("remote-mux-{name}"))
                .spawn(f)
                .map_err(|e| io::Error::new(e.kind(), format!("starting the mux's {name}: {e}")))
        };
        let s = shared.clone();
        let started = spawn("writer", Box::new(move || write_loop(&s, writer)))
            .and_then(|t| {
                threads.push(t);
                let s = shared.clone();
                spawn("reader", Box::new(move || read_loop(&s, reader)))
            })
            .and_then(|t| {
                threads.push(t);
                if beats {
                    let s = shared.clone();
                    threads.push(spawn("timer", Box::new(move || beat_loop(&s)))?);
                }
                Ok(())
            });
        if let Err(e) = started {
            // Undo the start: the threads that did start finish on the
            // lost link, the child is killed, and if no reader started to
            // reap it, it's reaped here.
            let child = {
                let mut st = shared.lock();
                shared.lose(&mut st, Lost::Io(e.to_string()));
                if threads.len() < 2 {
                    st.child.take()
                } else {
                    None
                }
            };
            if let Some(mut c) = child {
                let _ = c.wait();
            }
            return Err(e);
        }
        Ok(Mux { shared, threads })
    }

    /// Sends a request on a fixed channel. `op` must be the channel's op
    /// type (§ 2.2); the peer drops the link over one that isn't, so it's
    /// refused here first.
    pub fn request<T: Serialize + ?Sized>(&self, chan: u16, op: &T) -> Result<Stream, MuxError> {
        let mut bytes = frame::encode(Kind::Request, chan, 0, op)?;
        let header = Header {
            kind: Kind::Request,
            chan,
            id: 0,
            body_len: bytes.len() - HEADER,
        };
        check_body(&header, &bytes[HEADER..])?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        self.shared.gone(st)?;
        let origin = self.shared.role.origin();
        let id = loop {
            let n = st.next_id;
            st.next_id = n.wrapping_add(1) & !SERVER_ORIGINATED;
            if n != 0 && !st.routes.contains_key(&(chan, origin | n)) {
                break origin | n;
            }
        };
        bytes[8..12].copy_from_slice(&id.to_le_bytes());
        self.shared
            .open_route(st, (chan, id), RouteKind::Call, Holder::Live);
        let stream = Stream::new(self.shared.clone(), (chan, id), &st.routes[&(chan, id)]);
        self.shared.push(
            st,
            chan,
            Out {
                kind: Kind::Request,
                id,
                credit: 0,
                bytes,
            },
        );
        Ok(stream)
    }

    /// Claims the stream on dynamic channel `chan` (64 and up). What the
    /// peer sent on it before the claim is waiting in it, held to the
    /// channel's window. A channel carries one stream for the life of the
    /// link: claiming it again is [`MuxError::Spent`].
    pub fn stream(&self, chan: u16) -> Result<Stream, MuxError> {
        if chan < chan::FIRST_DYNAMIC {
            return Err(MuxError::NotStream(chan));
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        self.shared.gone(st)?;
        let key = (chan, 0);
        match st.routes.get_mut(&key) {
            Some(r) if r.holder == Holder::Unclaimed => r.holder = Holder::Live,
            Some(_) => return Err(MuxError::Taken(chan)),
            None if st.spent.contains(&chan) => return Err(MuxError::Spent(chan)),
            None => self
                .shared
                .open_route(st, key, RouteKind::Stream, Holder::Live),
        }
        Ok(Stream::new(self.shared.clone(), key, &st.routes[&key]))
    }

    /// The next request from the peer, waiting for one. `None` once the
    /// link is lost: requests not accepted by then are dropped unanswered.
    /// One task or thread should accept.
    pub fn accept(&self) -> Option<Incoming> {
        let mut st = self.shared.lock();
        loop {
            if st.lost.is_some() {
                return None;
            }
            if let Some(inc) = st.accept.pop_front() {
                return Some(inc);
            }
            st.wait(&self.shared.accept_cv);
        }
    }

    pub fn poll_accept(&self, cx: &mut Context<'_>) -> Poll<Option<Incoming>> {
        let mut st = self.shared.lock();
        if st.lost.is_some() {
            return Poll::Ready(None);
        }
        if let Some(inc) = st.accept.pop_front() {
            return Poll::Ready(Some(inc));
        }
        st.accept_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    pub async fn accept_async(&self) -> Option<Incoming> {
        poll_fn(|cx| self.poll_accept(cx)).await
    }

    /// Sizes the window this side grants `chan` from the link (§ 2.4):
    /// `2 × delivery rate × min RTT`, clamped, at each heartbeat. Off, the
    /// channel goes back to the configured window.
    pub fn set_bulk(&self, chan: u16, bulk: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let window = self.shared.window;
        let rw = self.shared.recv_window(st, chan);
        if bulk == rw.bulk.is_some() {
            return;
        }
        rw.bulk = bulk.then(DeliveryRate::default);
        rw.arrived = 0;
        if !bulk && let Some(grant) = rw.resize(window) {
            let f = frame::encode(Kind::Credit, chan, 0, &crate::Credit { bytes: grant })
                .expect("a credit encodes");
            self.shared.urgent(st, f);
        }
    }

    /// The window this side grants `chan` now.
    pub fn window(&self, chan: u16) -> u32 {
        let mut guard = self.shared.lock();
        self.shared.recv_window(&mut guard, chan).window()
    }

    /// The credit this side holds to send on `chan`.
    pub fn send_credit(&self, chan: u16) -> u64 {
        let mut guard = self.shared.lock();
        self.shared.send_chan(&mut guard, chan).credit
    }

    /// The heartbeat's round trips, once a pong has come back.
    pub fn rtt(&self) -> Option<Rtt> {
        self.shared.lock().heartbeat.as_ref()?.rtt()
    }

    pub fn lost(&self) -> Option<Lost> {
        self.shared.lock().lost.clone()
    }

    /// Waits for the link to be lost, and says why.
    pub fn wait_lost(&self) -> Lost {
        let mut st = self.shared.lock();
        loop {
            if let Some(lost) = &st.lost {
                return lost.clone();
            }
            st.wait(&self.shared.lost_cv);
        }
    }

    pub fn poll_lost(&self, cx: &mut Context<'_>) -> Poll<Lost> {
        let mut st = self.shared.lock();
        if let Some(lost) = &st.lost {
            return Poll::Ready(lost.clone());
        }
        state::register(&mut st.lost_wakers, cx);
        Poll::Pending
    }

    pub async fn lost_async(&self) -> Lost {
        poll_fn(|cx| self.poll_lost(cx)).await
    }

    fn hang_up(&self) {
        let mut st = self.shared.lock();
        st.closing = true;
        // Sends fail from now on: whoever waits for credit should see so.
        self.shared.wake_all_senders(&mut st);
        self.shared.writer_cv.notify_all();
        self.shared.alarm.stop();
    }

    /// Sends what's queued, hangs up, and waits for the peer to hang up in
    /// turn and the child, if any, to exit.
    pub fn close(mut self) {
        self.hang_up();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Mux {
    /// Hangs up without waiting: the threads finish on their own.
    fn drop(&mut self) {
        self.hang_up();
    }
}

fn write_loop(shared: &Shared, writer: Box<dyn Write + Send>) {
    let mut w = BufWriter::with_capacity(WRITE_BUFFER, writer);
    loop {
        let mut flushed = false;
        let mut st = shared.lock();
        let frame = loop {
            if st.lost.is_some() {
                break None;
            }
            if let Some(f) = st.out.pop() {
                break Some(f);
            }
            if st.closing {
                break None;
            }
            if !flushed {
                // Nothing to send: let what's buffered go before waiting.
                drop(st);
                if let Err(e) = w.flush() {
                    let mut st = shared.lock();
                    shared.lose(&mut st, Lost::Io(e.to_string()));
                    return;
                }
                flushed = true;
                st = shared.lock();
                continue;
            }
            st.wait(&shared.writer_cv);
        };
        drop(st);
        let Some(frame) = frame else {
            break;
        };
        if let Err(e) = w.write_all(&frame) {
            let mut st = shared.lock();
            shared.lose(&mut st, Lost::Io(e.to_string()));
            return;
        }
    }
    // Dropping the writer is the hang-up the peer waits for.
    let _ = w.flush();
}

fn read_loop(shared: &Arc<Shared>, mut reader: Box<dyn Read + Send>) {
    let reason = loop {
        match frame::read_frame(&mut reader) {
            Ok(Some((h, body))) => {
                if let Err(lost) = shared.on_frame(h, body) {
                    break lost;
                }
            }
            Ok(None) => break Lost::Eof,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                break Lost::Protocol(e.to_string());
            }
            Err(e) => break Lost::Io(e.to_string()),
        }
    };
    let child = {
        let mut st = shared.lock();
        let reason = if st.closing && reason == Lost::Eof {
            Lost::Closed
        } else {
            reason
        };
        shared.lose(&mut st, reason);
        st.child.take()
    };
    if let Some(mut c) = child {
        let _ = c.wait();
    }
}

fn beat_loop(shared: &Shared) {
    loop {
        let due = {
            let st = shared.lock();
            if st.lost.is_some() {
                return;
            }
            let Some(hb) = st.heartbeat.as_ref() else {
                return;
            };
            hb.due()
        };
        if !shared.alarm.wait_until(&shared.clock, due) {
            return;
        }
        let mut st = shared.lock();
        if st.lost.is_some() || st.closing {
            return;
        }
        shared.beat(&mut st);
    }
}
