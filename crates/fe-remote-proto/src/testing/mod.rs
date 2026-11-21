//! Fixtures for this crate's and other crates' tests (feature
//! `test-support`): the injected-latency link, a peer that speaks raw
//! frames, and a small service to run a mux against.

mod delayed;

pub use delayed::{Latency, delayed};

use crate::{CHUNK, Event, Incoming};
use crate::{
    ControlOp, FsData, FsOp, Header, Hello, IoError, IoKind, Kind, Link, Location, Mux, MuxError,
    OpenTarget, Opened, PROTOCOL, PathBytes, SessionToken, Status, Welcome, chan, frame,
};
use serde::Serialize;
use std::cell::Cell;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;

thread_local! {
    static SPAWNS_LEFT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Makes the next mux started on this thread fail to start its thread
/// after `started` of them have started, once.
pub fn fail_mux_threads_after(started: usize) {
    SPAWNS_LEFT.with(|s| s.set(Some(started)));
}

pub(crate) fn injected_spawn_failure() -> std::io::Result<()> {
    SPAWNS_LEFT.with(|s| match s.get() {
        Some(0) => {
            s.set(None);
            Err(std::io::Error::other("injected thread start failure"))
        }
        Some(n) => {
            s.set(Some(n - 1));
            Ok(())
        }
        None => Ok(()),
    })
}

/// The project a test's `Hello` names.
pub fn location() -> Location {
    Location {
        destination: "test".into(),
        path: PathBytes(b"/project".to_vec()),
    }
}

/// A server's accept callback that takes any `Hello` the mux passed.
pub fn accept_any(_: &Hello) -> Result<crate::Accept, String> {
    Ok(crate::Accept {
        session: SessionToken([7; 16]),
        streams: Vec::new(),
    })
}

/// One end of a link driven frame by frame, to see exactly what a mux
/// sends, or to play a peer that misbehaves. Every method panics on a
/// broken link: it's for tests.
pub struct RawPeer {
    pub reader: Box<dyn Read + Send>,
    pub writer: Option<Box<dyn Write + Send>>,
}

impl RawPeer {
    pub fn new(link: Link) -> RawPeer {
        RawPeer {
            reader: link.reader,
            writer: Some(link.writer),
        }
    }

    pub fn send<T: Serialize + ?Sized>(&mut self, kind: Kind, chan: u16, id: u32, body: &T) {
        self.send_frame(&frame::encode(kind, chan, id, body).unwrap());
    }

    pub fn send_raw(&mut self, kind: Kind, chan: u16, id: u32, body: &[u8]) {
        self.send_frame(&frame::encode_raw(kind, chan, id, body).unwrap());
    }

    fn send_frame(&mut self, f: &[u8]) {
        let w = self.writer.as_mut().expect("the raw peer hung up");
        w.write_all(f).unwrap();
        w.flush().unwrap();
    }

    /// Closes this side's writer: the mux reads EOF.
    pub fn hang_up(&mut self) {
        self.writer = None;
    }

    /// The next frame; `None` at a clean end of the link.
    pub fn recv(&mut self) -> Option<(Header, Vec<u8>)> {
        frame::read_frame(&mut self.reader).unwrap()
    }

    /// The next frame of `kind`, and the frames before it.
    pub fn recv_kind(&mut self, kind: Kind) -> ((Header, Vec<u8>), Vec<Header>) {
        let mut before = Vec::new();
        loop {
            let (h, b) = self.recv().expect("the link ended");
            if h.kind == kind {
                return ((h, b), before);
            }
            before.push(h);
        }
    }

    /// Plays a client: sends a `Hello` as `build` granting `window`, and
    /// returns the `Welcome`.
    pub fn hello(&mut self, build: &str, window: u32) -> Welcome {
        let hello = Hello {
            protocol: PROTOCOL,
            build: build.into(),
            window,
            resume: None,
            location: location(),
        };
        self.send(Kind::Hello, chan::CONTROL, 0, &hello);
        let (h, b) = self.recv().expect("no answer to the Hello");
        assert_eq!(h.kind, Kind::Welcome, "{h:?}");
        frame::body(&h, &b).unwrap()
    }

    /// Plays a server: takes the `Hello` and answers as `build` granting
    /// `window`, whatever the `Hello` said.
    pub fn welcome(&mut self, build: &str, window: u32) -> Hello {
        let (h, b) = self.recv().expect("no Hello");
        assert_eq!(h.kind, Kind::Hello, "{h:?}");
        let hello: Hello = frame::body(&h, &b).unwrap();
        let welcome = Welcome {
            protocol: PROTOCOL,
            build: build.into(),
            window,
            session: SessionToken([7; 16]),
            streams: Vec::new(),
        };
        self.send(Kind::Welcome, chan::CONTROL, 0, &welcome);
        hello
    }

    /// Pings with `nonce` and reads up to its pong: once it's back, the
    /// mux has handled every frame sent before the ping. Returns the frames
    /// read on the way.
    pub fn barrier(&mut self, nonce: u32) -> Vec<Header> {
        self.send(Kind::Ping, chan::CONTROL, nonce, &());
        let mut before = Vec::new();
        loop {
            let ((h, _), mut skipped) = self.recv_kind(Kind::Pong);
            before.append(&mut skipped);
            if h.id == nonce {
                return before;
            }
            before.push(h);
        }
    }
}

/// Serves test requests on `mux` until its link is lost, each on a thread
/// of its own (fine for a test; the mux itself has none per request):
///
/// - `FsOp::Read` of `endless` sends zero-filled chunks until cancelled;
///   any other path replies its own bytes, then `End(Ok)`.
/// - `ControlOp::Open(Tcp)` replies `Opened` with a fresh stream channel;
///   on it, host `echo` sends back what it gets, and host `bulk` sends
///   zero-filled chunks until cancelled.
pub fn serve(mux: &Mux) {
    let next_chan = AtomicU16::new(chan::FIRST_DYNAMIC);
    while let Some(inc) = mux.accept() {
        // The stream is claimed here, before the reply names it, so
        // nothing the client sends on it can arrive unclaimed.
        let opened = (inc.header.chan == chan::CONTROL).then(|| open(mux, &inc, &next_chan));
        thread::spawn(move || answer(inc, opened));
    }
}

/// What an `Open` made: the new stream, and what it should do.
enum Opening {
    Echo(crate::Stream),
    Bulk(crate::Stream),
    Refused,
}

fn open(mux: &Mux, inc: &Incoming, next: &AtomicU16) -> Opening {
    let Ok(ControlOp::Open(OpenTarget::Tcp { host, .. })) = inc.op::<ControlOp>() else {
        return Opening::Refused;
    };
    let make: fn(crate::Stream) -> Opening = match host.as_str() {
        "echo" => Opening::Echo,
        "bulk" => Opening::Bulk,
        _ => return Opening::Refused,
    };
    let chan = next.fetch_add(1, Ordering::Relaxed);
    mux.stream(chan).map_or(Opening::Refused, make)
}

fn unsupported() -> Status {
    Status::Error(IoError {
        kind: IoKind::Unsupported,
        message: "not a test op".into(),
    })
}

/// Sends until the peer cancels or the link goes.
fn flood(mut send: impl FnMut() -> Result<(), MuxError>) -> Status {
    loop {
        match send() {
            Ok(()) => {}
            Err(MuxError::Cancelled) => return Status::Cancelled,
            Err(_) => return Status::LinkLost,
        }
    }
}

fn answer(inc: Incoming, opened: Option<Opening>) {
    let reply = &inc.stream;
    let status = match (inc.header.chan, opened) {
        (chan::FS, _) => match inc.op::<FsOp>() {
            Ok(FsOp::Read { path }) if path.0 == b"endless" => {
                let chunk = FsData::Bytes(vec![0; CHUNK]);
                flood(|| reply.send(&chunk))
            }
            Ok(FsOp::Read { path }) => {
                let sent = path
                    .0
                    .chunks(CHUNK)
                    .try_for_each(|c| reply.send(&FsData::Bytes(c.to_vec())));
                if sent.is_ok() {
                    Status::Ok
                } else {
                    Status::Cancelled
                }
            }
            _ => unsupported(),
        },
        (chan::CONTROL, Some(Opening::Echo(stream))) => {
            let _ = reply.send(&Opened {
                chan: stream.chan(),
            });
            thread::spawn(move || echo(stream));
            Status::Ok
        }
        (chan::CONTROL, Some(Opening::Bulk(stream))) => {
            let _ = reply.send(&Opened {
                chan: stream.chan(),
            });
            thread::spawn(move || {
                let chunk = vec![0; CHUNK];
                let status = flood(|| stream.send_bytes(&chunk));
                let _ = stream.end(status);
            });
            Status::Ok
        }
        _ => unsupported(),
    };
    let _ = reply.end(status);
}

fn echo(mut stream: crate::Stream) {
    while let Some(ev) = stream.recv() {
        match ev {
            Event::Data(bytes) => {
                if stream.send_bytes(&bytes).is_err() {
                    break;
                }
            }
            Event::End(status) => {
                let _ = stream.end(status);
                return;
            }
        }
    }
}
