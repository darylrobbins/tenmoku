//! The spike client: a connection multiplexing requests over one link.
//!
//! A reader thread routes each frame to its request's stream by
//! `(channel, request id)`. A stream returns credit to its channel as its
//! consumer takes data, so the server can never run more than one window
//! ahead of a slow consumer on that channel, and other channels keep moving.

use crate::frame::{self, Body, Hello, Kind, Op, PROTOCOL, Status};
use crate::transport::{Link, Transport};
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Child;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub enum Event {
    /// A body and its size on the wire, which is what credit counts.
    Data(Body, usize),
    End(Status),
}

struct Inner {
    writer: Mutex<Box<dyn Write + Send>>,
    routes: Mutex<HashMap<(u16, u32), Sender<Event>>>,
    pings: Mutex<HashMap<u32, Sender<()>>>,
    owed: Mutex<HashMap<u16, u32>>,
    next_id: AtomicU32,
    window: u32,
}

impl Inner {
    fn send(&self, f: &[u8]) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(f)?;
        w.flush()
    }

    /// Returns `n` bytes of credit to `chan`, in batches of a quarter window
    /// (or at once with `now`), so credit frames stay rare.
    fn credit(&self, chan: u16, n: usize, now: bool) {
        let grant = {
            let mut owed = self.owed.lock().unwrap();
            let o = owed.entry(chan).or_default();
            *o += n as u32;
            if *o > 0 && (now || *o >= self.window / 4) {
                std::mem::take(o)
            } else {
                0
            }
        };
        if grant > 0 {
            let _ = self.send(&frame::encode(Kind::Credit, chan, 0, &grant));
        }
    }
}

pub struct Conn {
    inner: Arc<Inner>,
    child: Option<Child>,
    reader: Option<thread::JoinHandle<()>>,
    pub server_build: String,
}

impl Conn {
    /// Opens the transport and runs the handshake. A protocol or build
    /// mismatch comes back as an error carrying the server's reason.
    pub fn connect(t: &dyn Transport, build: &str, window: u32) -> io::Result<Conn> {
        let Link {
            mut reader,
            mut writer,
            child,
        } = t.open()?;
        let hello = Hello {
            protocol: PROTOCOL,
            build: build.to_string(),
            window,
        };
        writer.write_all(&frame::encode(Kind::Hello, 0, 0, &hello))?;
        writer.flush()?;
        let (h, b) = frame::read_frame(&mut reader)?
            .ok_or_else(|| io::Error::other("server closed during the handshake"))?;
        let welcome: Hello = match h.kind {
            Kind::Welcome => frame::body(&b)?,
            Kind::Refused => {
                let why: String = frame::body(&b)?;
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, why));
            }
            k => return Err(io::Error::other(format!("handshake got {k:?}"))),
        };
        // Both peers check: a server that accepted the Hello but is another
        // protocol or build (a stale or hand-built binary) is refused here.
        if welcome.protocol != PROTOCOL || welcome.build != build {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "server answered protocol {} build {:?}, client is protocol {PROTOCOL} build {build:?}",
                    welcome.protocol, welcome.build
                ),
            ));
        }
        let inner = Arc::new(Inner {
            writer: Mutex::new(writer),
            routes: Mutex::default(),
            pings: Mutex::default(),
            owed: Mutex::default(),
            next_id: AtomicU32::new(1),
            window,
        });
        let reader = {
            let inner = inner.clone();
            thread::spawn(move || {
                while let Ok(Some((h, b))) = frame::read_frame(&mut reader) {
                    match h.kind {
                        Kind::Data => {
                            let Ok(body) = frame::body::<Body>(&b) else {
                                break;
                            };
                            let routes = inner.routes.lock().unwrap();
                            if let Some(tx) = routes.get(&(h.chan, h.id)) {
                                let _ = tx.send(Event::Data(body, h.body_len));
                            } else {
                                // Nobody is listening: still return the credit.
                                drop(routes);
                                inner.credit(h.chan, h.body_len, false);
                            }
                        }
                        Kind::End => {
                            let Ok(status) = frame::body::<Status>(&b) else {
                                break;
                            };
                            if let Some(tx) = inner.routes.lock().unwrap().remove(&(h.chan, h.id)) {
                                let _ = tx.send(Event::End(status));
                            }
                        }
                        Kind::Pong => {
                            if let Some(tx) = inner.pings.lock().unwrap().remove(&h.id) {
                                let _ = tx.send(());
                            }
                        }
                        _ => break,
                    }
                }
                // Link gone: every open stream sees its sender drop.
                inner.routes.lock().unwrap().clear();
                inner.pings.lock().unwrap().clear();
            })
        };
        Ok(Conn {
            inner,
            child,
            reader: Some(reader),
            server_build: welcome.build,
        })
    }

    pub fn request(&self, chan: u16, op: &Op) -> io::Result<Stream> {
        assert!(chan != 0, "channel 0 is control");
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.inner.routes.lock().unwrap().insert((chan, id), tx);
        self.inner
            .send(&frame::encode(Kind::Request, chan, id, op))?;
        Ok(Stream {
            inner: self.inner.clone(),
            chan,
            id,
            rx,
            ended: false,
        })
    }

    /// One heartbeat round trip on the control channel.
    pub fn ping(&self) -> io::Result<Duration> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.inner.pings.lock().unwrap().insert(id, tx);
        let t = Instant::now();
        self.inner.send(&frame::encode(Kind::Ping, 0, id, &()))?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?;
        Ok(t.elapsed())
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // Closing our end of the pipe is the hang-up the server waits for.
        *self.inner.writer.lock().unwrap() = Box::new(io::sink());
        if let Some(mut c) = self.child.take() {
            let _ = c.wait();
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

pub struct Stream {
    inner: Arc<Inner>,
    chan: u16,
    id: u32,
    rx: Receiver<Event>,
    ended: bool,
}

impl Stream {
    /// The next event; `None` after the end, or if the link dropped.
    pub fn recv(&mut self) -> Option<Event> {
        if self.ended {
            return None;
        }
        let ev = self.rx.recv().ok();
        match &ev {
            Some(Event::Data(_, n)) => self.inner.credit(self.chan, *n, false),
            Some(Event::End(_)) => {
                self.ended = true;
                self.inner.credit(self.chan, 0, true);
            }
            None => self.ended = true,
        }
        ev
    }

    /// Asks the server to stop, then drains to the end. Returns the status
    /// and what arrived after the cancel was sent: wire bytes and entries.
    pub fn cancel(&mut self) -> io::Result<(Option<Status>, usize, usize)> {
        self.inner
            .send(&frame::encode(Kind::Cancel, self.chan, self.id, &()))?;
        let (mut bytes, mut entries) = (0, 0);
        while let Some(ev) = self.recv() {
            match ev {
                Event::Data(body, n) => {
                    bytes += n;
                    if let Body::Entries(e) = body {
                        entries += e.len();
                    }
                }
                Event::End(s) => return Ok((Some(s), bytes, entries)),
            }
        }
        Ok((None, bytes, entries))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.inner
            .routes
            .lock()
            .unwrap()
            .remove(&(self.chan, self.id));
        // Data already routed here was never consumed: hand its credit back.
        while let Ok(ev) = self.rx.try_recv() {
            if let Event::Data(_, n) = ev {
                self.inner.credit(self.chan, n, false);
            }
        }
        self.inner.credit(self.chan, 0, true);
    }
}
