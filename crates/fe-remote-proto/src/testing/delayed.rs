//! `Delayed` (D § 12): a link with injected one-way latency each way and,
//! optionally, a bandwidth cap, so the protocol can be measured as if over
//! a network. Ported from `tools/remote-spike/src/transport.rs`.

use crate::Link;
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The network a [`delayed`] link pretends to be.
#[derive(Clone, Copy, Debug)]
pub struct Latency {
    /// Added to each direction.
    pub one_way: Duration,
    /// Each direction's bandwidth; `None` for no cap.
    pub bytes_per_sec: Option<f64>,
}

impl Latency {
    pub fn one_way(one_way: Duration) -> Latency {
        Latency {
            one_way,
            bytes_per_sec: None,
        }
    }
}

/// Wraps both directions of `link` in `latency`. Bytes are pipelined: a
/// write leaves the "wire" once the bytes ahead of it have (at the capped
/// rate) and arrives `one_way` later, however many writes are in flight.
/// Writes never block, as bytes handed to ssh don't: the wire is the queue.
pub fn delayed(link: Link, latency: Latency) -> Link {
    if latency.one_way.is_zero() && latency.bytes_per_sec.is_none() {
        return link;
    }
    let wire = Wire {
        latency,
        busy_until: None,
    };
    Link {
        reader: Box::new(DelayedReader::new(link.reader, wire)),
        writer: Box::new(DelayedWriter::new(link.writer, wire)),
        child: link.child,
    }
}

/// When bytes put on the wire now arrive: once the bytes queued ahead have
/// gone, plus the latency.
#[derive(Clone, Copy)]
struct Wire {
    latency: Latency,
    busy_until: Option<Instant>,
}

impl Wire {
    fn arrival(&mut self, len: usize) -> Instant {
        let now = Instant::now();
        let Some(rate) = self.latency.bytes_per_sec else {
            return now + self.latency.one_way;
        };
        let start = self.busy_until.map_or(now, |b| b.max(now));
        let sent = start + Duration::from_secs_f64(len as f64 / rate);
        self.busy_until = Some(sent);
        sent + self.latency.one_way
    }
}

type Stamped = (Instant, Vec<u8>);

/// Waits until `t` by yielding, not sleeping: on the Mac the spike was
/// measured on, `thread::sleep(5 ms)` took 40 ms under load (timer
/// coalescing), which would inflate every injected delay eightfold. The
/// relay threads spin only while bytes are in flight.
fn spin_until(t: Instant) {
    while Instant::now() < t {
        thread::yield_now();
    }
}

struct DelayedWriter {
    tx: Option<mpsc::Sender<Stamped>>,
    pump: Option<thread::JoinHandle<()>>,
    wire: Wire,
}

impl DelayedWriter {
    fn new(mut inner: Box<dyn Write + Send>, wire: Wire) -> DelayedWriter {
        let (tx, rx) = mpsc::channel::<Stamped>();
        let pump = thread::spawn(move || {
            while let Ok((at, bytes)) = rx.recv() {
                spin_until(at);
                if inner
                    .write_all(&bytes)
                    .and_then(|()| inner.flush())
                    .is_err()
                {
                    return;
                }
            }
        });
        DelayedWriter {
            tx: Some(tx),
            pump: Some(pump),
            wire,
        }
    }
}

impl Write for DelayedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let tx = self.tx.as_ref().ok_or(io::ErrorKind::BrokenPipe)?;
        tx.send((self.wire.arrival(buf.len()), buf.to_vec()))
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for DelayedWriter {
    fn drop(&mut self) {
        // Closing the channel lets the pump deliver what's in flight and
        // then drop (close) the pipe.
        self.tx.take();
        if let Some(p) = self.pump.take() {
            let _ = p.join();
        }
    }
}

struct DelayedReader {
    rx: mpsc::Receiver<Stamped>,
    pending: Vec<u8>,
    at: usize,
}

impl DelayedReader {
    fn new(mut inner: Box<dyn Read + Send>, mut wire: Wire) -> DelayedReader {
        let (tx, rx) = mpsc::channel::<Stamped>();
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match inner.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if tx.send((wire.arrival(n), buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        DelayedReader {
            rx,
            pending: Vec::new(),
            at: 0,
        }
    }
}

impl Read for DelayedReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at == self.pending.len() {
            match self.rx.recv() {
                Ok((at, bytes)) => {
                    spin_until(at);
                    self.pending = bytes;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.pending.len() - self.at);
        out[..n].copy_from_slice(&self.pending[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}
