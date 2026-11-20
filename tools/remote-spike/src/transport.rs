//! Transports: something that yields a byte pipe to a server.
//!
//! The real client has one impl per kind of host (ssh in v1, `wsl.exe -d`
//! later, `docker exec -i` after v1); each is a child process whose stdio is
//! the pipe. `Delayed` wraps any transport with injected one-way latency, so
//! the protocol can be measured as if over a network.

use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub struct Link {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Option<Child>,
}

pub trait Transport {
    fn open(&self) -> io::Result<Link>;
}

/// A child process speaking the protocol on its stdin and stdout, for example
/// `ssh -T host tenmoku-server --stdio`, or this binary's `server` mode.
pub struct ChildStdio {
    pub program: String,
    pub args: Vec<String>,
}

impl Transport for ChildStdio {
    fn open(&self) -> io::Result<Link> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let writer = Box::new(child.stdin.take().unwrap());
        let reader = Box::new(child.stdout.take().unwrap());
        Ok(Link {
            reader,
            writer,
            child: Some(child),
        })
    }
}

/// Adds `one_way` latency to each direction of another transport, and with
/// `bytes_per_sec` a bandwidth cap. Bytes are pipelined: a write leaves the
/// "wire" once the bytes ahead of it have (at the capped rate) and arrives
/// `one_way` later, however many writes are in flight.
pub struct Delayed<T> {
    pub inner: T,
    pub one_way: Duration,
    pub bytes_per_sec: Option<f64>,
}

/// When bytes written now arrive: after the queue ahead drains, plus latency.
#[derive(Clone, Copy)]
struct Wire {
    one_way: Duration,
    bytes_per_sec: Option<f64>,
    busy_until: Option<Instant>,
}

impl Wire {
    fn arrival(&mut self, len: usize) -> Instant {
        let now = Instant::now();
        let Some(rate) = self.bytes_per_sec else {
            return now + self.one_way;
        };
        let start = self.busy_until.map_or(now, |b| b.max(now));
        let sent = start + Duration::from_secs_f64(len as f64 / rate);
        self.busy_until = Some(sent);
        sent + self.one_way
    }
}

impl<T: Transport> Transport for Delayed<T> {
    fn open(&self) -> io::Result<Link> {
        let link = self.inner.open()?;
        if self.one_way.is_zero() && self.bytes_per_sec.is_none() {
            return Ok(link);
        }
        let wire = Wire {
            one_way: self.one_way,
            bytes_per_sec: self.bytes_per_sec,
            busy_until: None,
        };
        Ok(Link {
            reader: Box::new(DelayedReader::new(link.reader, wire)),
            writer: Box::new(DelayedWriter::new(link.writer, wire)),
            child: link.child,
        })
    }
}

type Stamped = (Instant, Vec<u8>);

/// Waits until `t` by yielding, not sleeping: on the Mac the spike was
/// measured on, `thread::sleep(5 ms)` took 40 ms under load (timer
/// coalescing), which would inflate every injected delay eightfold. The
/// relay threads only wait while bytes are in flight.
fn sleep_until(t: Instant) {
    while Instant::now() < t {
        thread::yield_now();
    }
}

pub struct DelayedWriter {
    tx: Option<mpsc::Sender<Stamped>>,
    pump: Option<thread::JoinHandle<()>>,
    wire: Wire,
}

impl DelayedWriter {
    fn new(mut inner: Box<dyn Write + Send>, wire: Wire) -> Self {
        let (tx, rx) = mpsc::channel::<Stamped>();
        let pump = thread::spawn(move || {
            while let Ok((at, bytes)) = rx.recv() {
                sleep_until(at);
                if inner.write_all(&bytes).and_then(|_| inner.flush()).is_err() {
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
        let tx = self.tx.as_ref().unwrap();
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
        // Closing the channel lets the pump drain and then drop (close) the pipe.
        self.tx.take();
        if let Some(p) = self.pump.take() {
            let _ = p.join();
        }
    }
}

pub struct DelayedReader {
    rx: mpsc::Receiver<Stamped>,
    pending: Vec<u8>,
    at: usize,
}

impl DelayedReader {
    fn new(mut inner: Box<dyn Read + Send>, mut wire: Wire) -> Self {
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
                    sleep_until(at);
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
