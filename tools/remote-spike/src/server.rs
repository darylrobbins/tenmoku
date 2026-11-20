//! The spike server: `remote-spike server` speaks the protocol on its stdio.
//!
//! One reader (the calling thread), one writer thread, and a worker thread per
//! request. Workers take per-channel credit before queueing a data frame, so
//! no channel can queue more than the client's window. The writer sends
//! urgent frames (handshake, pongs) first, then round-robins the channels, so
//! a bulk stream on one channel delays another by at most one chunk.

use crate::frame::{self, Body, CHUNK, Entry, Hello, Kind, Op, PROTOCOL, Status};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::UNIX_EPOCH;

/// Entries per walk batch, or fewer once their paths pass `BATCH_PATH_BYTES`.
pub const BATCH_ENTRIES: usize = 512;
const BATCH_PATH_BYTES: usize = 32 * 1024;

#[derive(Default)]
struct OutQ {
    urgent: VecDeque<Vec<u8>>,
    chans: BTreeMap<u16, VecDeque<Vec<u8>>>,
    last: u16,
    closed: bool,
}

#[derive(Default)]
struct Outbox {
    q: Mutex<OutQ>,
    cv: Condvar,
}

impl Outbox {
    fn urgent(&self, f: Vec<u8>) {
        self.q.lock().unwrap().urgent.push_back(f);
        self.cv.notify_one();
    }

    fn push(&self, chan: u16, f: Vec<u8>) {
        self.q
            .lock()
            .unwrap()
            .chans
            .entry(chan)
            .or_default()
            .push_back(f);
        self.cv.notify_one();
    }

    fn close(&self) {
        self.q.lock().unwrap().closed = true;
        self.cv.notify_all();
    }

    /// The next frame, or `None` once closed and drained. Flushes `w` before
    /// it waits, so a quiet link never holds a buffered frame back.
    fn pop(&self, w: &mut impl Write) -> io::Result<Option<Vec<u8>>> {
        let mut q = self.q.lock().unwrap();
        loop {
            if let Some(f) = q.urgent.pop_front() {
                return Ok(Some(f));
            }
            let after = q.last;
            let pick = q
                .chans
                .range(after.wrapping_add(1)..)
                .chain(q.chans.range(..=after))
                .find(|(_, v)| !v.is_empty())
                .map(|(c, _)| *c);
            if let Some(c) = pick {
                q.last = c;
                return Ok(q.chans.get_mut(&c).unwrap().pop_front());
            }
            if q.closed {
                return Ok(None);
            }
            drop(q);
            w.flush()?;
            q = self.q.lock().unwrap();
            if q.urgent.is_empty() && q.chans.values().all(VecDeque::is_empty) && !q.closed {
                q = self.cv.wait(q).unwrap();
            }
        }
    }
}

struct Credits {
    m: Mutex<HashMap<u16, i64>>,
    cv: Condvar,
    window: i64,
}

impl Credits {
    /// Waits for `n` bytes of credit on `chan`; false if cancelled first.
    fn take(&self, chan: u16, n: usize, cancel: &AtomicBool) -> bool {
        let mut m = self.m.lock().unwrap();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            let c = m.entry(chan).or_insert(self.window);
            if *c >= n as i64 {
                *c -= n as i64;
                return true;
            }
            m = self.cv.wait(m).unwrap();
        }
    }

    fn grant(&self, chan: u16, n: u32) {
        *self.m.lock().unwrap().entry(chan).or_insert(self.window) += n as i64;
        self.cv.notify_all();
    }
}

struct Shared {
    out: Outbox,
    credits: Credits,
    cancels: Mutex<HashMap<(u16, u32), Arc<AtomicBool>>>,
}

/// Serves one connection until the reader ends. `build` must match the
/// client's exactly.
pub fn serve(mut r: impl Read, w: impl Write + Send + 'static, build: &str) -> io::Result<()> {
    let mut w = io::BufWriter::with_capacity(256 * 1024, w);
    let Some((h, b)) = frame::read_frame(&mut r)? else {
        return Ok(());
    };
    let hello: Hello = if h.kind == Kind::Hello {
        frame::body(&b)?
    } else {
        return Err(io::Error::other("first frame is not Hello"));
    };
    if hello.protocol != PROTOCOL || hello.build != build {
        let why = format!(
            "client protocol {} build {:?}, server protocol {PROTOCOL} build {build:?}",
            hello.protocol, hello.build
        );
        w.write_all(&frame::encode(Kind::Refused, 0, 0, &why))?;
        return w.flush();
    }
    if (hello.window as usize) < 4 * CHUNK {
        let why = format!("window {} is under 4 chunks", hello.window);
        w.write_all(&frame::encode(Kind::Refused, 0, 0, &why))?;
        return w.flush();
    }
    let welcome = Hello {
        protocol: PROTOCOL,
        build: build.to_string(),
        window: hello.window,
    };
    w.write_all(&frame::encode(Kind::Welcome, 0, 0, &welcome))?;
    w.flush()?;

    let shared = Arc::new(Shared {
        out: Outbox::default(),
        credits: Credits {
            m: Mutex::default(),
            cv: Condvar::new(),
            window: hello.window as i64,
        },
        cancels: Mutex::default(),
    });
    let writer = {
        let shared = shared.clone();
        thread::spawn(move || -> io::Result<()> {
            while let Some(f) = shared.out.pop(&mut w)? {
                w.write_all(&f)?;
            }
            w.flush()
        })
    };

    while let Some((h, b)) = frame::read_frame(&mut r)? {
        match h.kind {
            Kind::Request => {
                let op: Op = frame::body(&b)?;
                let cancel = Arc::new(AtomicBool::new(false));
                shared
                    .cancels
                    .lock()
                    .unwrap()
                    .insert((h.chan, h.id), cancel.clone());
                let shared = shared.clone();
                thread::spawn(move || run(&shared, h.chan, h.id, op, &cancel));
            }
            Kind::Cancel => {
                if let Some(c) = shared.cancels.lock().unwrap().get(&(h.chan, h.id)) {
                    c.store(true, Ordering::Relaxed);
                }
                // Wake a worker parked on credit so it sees the flag.
                let _guard = shared.credits.m.lock().unwrap();
                shared.credits.cv.notify_all();
            }
            Kind::Credit => shared.credits.grant(h.chan, frame::body(&b)?),
            Kind::Ping => shared.out.urgent(frame::encode(Kind::Pong, 0, h.id, &())),
            other => return Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }
    // The client hung up: stop every worker, then let the writer drain.
    for c in shared.cancels.lock().unwrap().values() {
        c.store(true, Ordering::Relaxed);
    }
    shared.credits.cv.notify_all();
    shared.out.close();
    writer.join().unwrap()
}

fn run(s: &Shared, chan: u16, id: u32, op: Op, cancel: &AtomicBool) {
    let send = |body: Body| -> bool {
        let f = frame::encode(Kind::Data, chan, id, &body);
        let n = f.len() - frame::HEADER;
        if !s.credits.take(chan, n, cancel) {
            return false;
        }
        s.out.push(chan, f);
        true
    };
    let status = match op {
        // Chunked like any other reply, so an echo larger than the window
        // still completes as the consumer returns credit.
        Op::Echo(bytes) if bytes.is_empty() => Ok(send(Body::Bytes(bytes))),
        Op::Echo(bytes) => Ok(bytes.chunks(CHUNK).all(|c| send(Body::Bytes(c.to_vec())))),
        Op::ReadFile { path } => read_file(Path::new(&path), &send),
        Op::Walk { root } => walk(Path::new(&root), &mut |batch| send(Body::Entries(batch))),
        Op::Bulk { bytes } => {
            let chunk = vec![0u8; CHUNK];
            let mut left = bytes;
            let mut sent = true;
            while left > 0 && sent {
                let n = left.min(CHUNK as u64) as usize;
                sent = send(Body::Bytes(chunk[..n].to_vec()));
                left -= n as u64;
            }
            Ok(sent)
        }
    };
    let status = match status {
        Ok(true) => Status::Ok,
        Ok(false) => Status::Cancelled,
        Err(e) => Status::Error(e.to_string()),
    };
    let status = if cancel.load(Ordering::Relaxed) {
        Status::Cancelled
    } else {
        status
    };
    s.cancels.lock().unwrap().remove(&(chan, id));
    s.out
        .push(chan, frame::encode(Kind::End, chan, id, &status));
}

fn read_file(path: &Path, send: &dyn Fn(Body) -> bool) -> io::Result<bool> {
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(true);
        }
        if !send(Body::Bytes(buf[..n].to_vec())) {
            return Ok(false);
        }
    }
}

/// Walks `root` depth first, lstat-ing every entry, and hands out batches.
/// `emit` returns false to stop (cancelled). Paths are relative to `root`.
pub fn walk(root: &Path, emit: &mut dyn FnMut(Vec<Entry>) -> bool) -> io::Result<bool> {
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    let mut batch = Vec::with_capacity(BATCH_ENTRIES);
    let mut path_bytes = 0;
    while let Some(rel) = stack.pop() {
        let Ok(dir) = fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for e in dir.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            let path = rel.join(e.file_name());
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i64);
            if meta.is_dir() {
                stack.push(path.clone());
            }
            let path = path.to_string_lossy().into_owned();
            path_bytes += path.len();
            batch.push(Entry {
                path,
                dir: meta.is_dir(),
                size: meta.len(),
                mtime_ns,
            });
            if batch.len() == BATCH_ENTRIES || path_bytes >= BATCH_PATH_BYTES {
                path_bytes = 0;
                if !emit(std::mem::take(&mut batch)) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(batch.is_empty() || emit(batch))
}
