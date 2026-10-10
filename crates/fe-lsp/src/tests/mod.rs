//! Unit tests, one module per area. They run servers through the fake in
//! `crate::fake` (declared in lib.rs, so fe-workbench can share it).

mod admission;
mod cancel;
mod completion;
mod diagnostics;
mod encoding;
mod hover;
mod identity;
mod kill;
mod launcher;
mod launcher_path_safety;
mod lifecycle;
mod navigation;
mod readiness;
mod registration;
mod registry;
mod restart;
mod router;
mod status;
mod sync;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use fe_buffer::{Buffer, Origin};
use futures::FutureExt;
use lsp_types::Url;
use serde_json::{Value, json};

use crate::fake::{Echo, FakeServer, Record, Replica, Script};
use crate::{
    Clock, LspEvent, LspManager, ManualClock, ServerId, ServerLauncher, ServerSpec, ServerStatus,
    SystemClock,
};

/// How long a test waits for anything before failing.
const WAIT: Duration = Duration::from_secs(10);

/// A manager whose events land in the returned [`Events`].
fn manager(
    launcher: Arc<dyn ServerLauncher>,
    clock: Option<Arc<dyn Clock>>,
) -> (LspManager, Events) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let probe = Probe::default();
    let sink = {
        let probe = probe.clone();
        move |event| {
            if let Some(probe) = &mut *probe.lock().unwrap_or_else(PoisonError::into_inner) {
                probe(&event);
            }
            let _ = sender.send(event);
        }
    };
    let manager = match clock {
        Some(clock) => LspManager::with_clock(launcher, clock, sink),
        None => LspManager::new(launcher, sink),
    };
    (manager, Events(receiver, probe))
}

/// Sees each event in the sink, on the server's thread, as it's reported.
type Probe = Arc<Mutex<Option<Box<dyn FnMut(&LspEvent) + Send>>>>;

struct Events(Receiver<LspEvent>, Probe);

impl Events {
    /// Calls `probe` with each event from now on, in the sink.
    fn probe(&self, probe: impl FnMut(&LspEvent) + Send + 'static) {
        *self.1.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(probe));
    }

    /// The next status `server` reports.
    fn status(&self, server: ServerId) -> ServerStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.0.recv_timeout(left) {
                Ok(LspEvent::Status {
                    server: from,
                    status,
                    ..
                }) if from == server => return status,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("no status from {server:?} in {WAIT:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the manager has gone"),
            }
        }
    }

    /// Waits for the next event that `pick` takes.
    fn until<T>(&self, what: &str, mut pick: impl FnMut(LspEvent) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.0.recv_timeout(left) {
                Ok(event) => {
                    if let Some(picked) = pick(event) {
                        return picked;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no {what} in {WAIT:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the manager has gone"),
            }
        }
    }

    /// Asserts nothing arrives for a moment of real time.
    fn assert_quiet(&self) {
        if let Ok(event) = self.0.recv_timeout(Duration::from_millis(100)) {
            panic!("unexpected {event:?}");
        }
    }
}

const ROOT: &str = "/fake/root";

/// A server started on the fake, with a manual clock that never moves, so
/// no backstop tick drains behind a test's back.
struct Started {
    fake: FakeServer,
    manager: LspManager,
    server: ServerId,
    events: Events,
}

impl Started {
    fn new(script: Script) -> Self {
        let fake = FakeServer::new(script);
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new());
        let (mut manager, events) = manager(fake.launcher(), Some(clock));
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        assert_eq!(events.status(server), ServerStatus::Starting);
        fake.wait_initialized();
        Self {
            fake,
            manager,
            server,
            events,
        }
    }

    /// Opens `buffer` as `name` under the root on `server`, without
    /// waiting for anything.
    fn open_on(&mut self, server: ServerId, buffer: &mut Buffer, name: &str) -> Url {
        let path = PathBuf::from(ROOT).join(name);
        let subscription = buffer.subscribe();
        self.manager
            .open_on(
                server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &path,
                "text",
            )
            .unwrap();
        Url::from_file_path(&path).unwrap()
    }

    fn open(&mut self, buffer: &mut Buffer, name: &str) -> Url {
        self.open_on(self.server, buffer, name)
    }

    /// Opens `buffer` and waits for its `didOpen`.
    fn open_synced(&mut self, buffer: &mut Buffer, name: &str) -> Url {
        let uri = self.open(buffer, name);
        self.fake
            .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
        uri
    }

    /// Edits `buffer`, nudges its server and waits until the replica at
    /// `uri` has `version`.
    fn edit(
        &self,
        buffer: &mut Buffer,
        uri: &Url,
        edit: (std::ops::Range<usize>, &str),
        version: i32,
    ) -> Replica {
        buffer
            .edit([edit], Origin::User)
            .expect("the edit changed something");
        self.manager.changed(buffer.id());
        self.fake
            .wait_for_document(uri, &format!("version {version}"), |r| {
                r.is_some_and(|r| r.version >= version)
            })
            .unwrap()
    }

    /// Returns once the fake has read everything the actor sent before
    /// now: a request goes behind it, and the fake answers in order.
    fn barrier(&self) {
        answer(self.manager.request::<Echo>(self.server, json!({}))).unwrap();
    }

    /// The document notifications the fake got for `uri`, in order.
    fn notes(&self, uri: &Url) -> Vec<(String, Value)> {
        self.fake
            .records()
            .into_iter()
            .filter_map(|record| match record {
                Record::Notification { method, params }
                    if params["textDocument"]["uri"] == json!(uri) =>
                {
                    Some((method, params))
                }
                _ => None,
            })
            .collect()
    }

    /// The methods of [`notes`](Self::notes), without `textDocument/`.
    fn methods(&self, uri: &Url) -> Vec<String> {
        self.notes(uri)
            .into_iter()
            .map(|(method, _)| method["textDocument/".len()..].to_owned())
            .collect()
    }
}

/// A server started from `spec` on the fake, with a manual clock, and
/// initialized.
struct Clocked {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    manager: LspManager,
    server: ServerId,
    events: Events,
}

impl Clocked {
    fn new(spec: ServerSpec, script: Script) -> Self {
        let fake = FakeServer::new(script);
        let clock = Arc::new(ManualClock::new());
        let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
        let server = manager.start(spec, Path::new(ROOT)).unwrap();
        assert_eq!(events.status(server), ServerStatus::Starting);
        fake.wait_initialized();
        Self {
            fake,
            clock,
            manager,
            server,
            events,
        }
    }

    /// Asserts `server`'s next status is `status`.
    fn expect(&self, status: ServerStatus) {
        assert_eq!(self.events.status(self.server), status);
    }

    /// Lets readiness's 500 ms pass and sees the server `Ready`.
    fn settle(&self) {
        self.clock.wait_for_sleep(crate::readiness::SETTLE);
        self.clock.advance(crate::readiness::SETTLE);
        self.expect(ServerStatus::Ready);
    }
}

/// Waits for `reply`; panics after [`WAIT`] of real time.
fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = SystemClock.sleep(WAIT).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in {WAIT:?}"),
        }
    })
}

/// Waits for `done`, polling; panics after [`WAIT`] naming `what`.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every log record so far from the thread named `thread`, which
/// [`capture_logs`] must have named.
fn logs_from(thread: &str) -> Vec<String> {
    LOGGER
        .get_or_init(install_logger)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .records
        .iter()
        .filter(|(from, _)| from == thread)
        .map(|(_, message)| message.clone())
        .collect()
}

/// Starts capturing the log records of the thread named `thread`; call
/// before the code under test runs. Other threads' records are dropped.
fn capture_logs(thread: &str) {
    LOGGER
        .get_or_init(install_logger)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .threads
        .push(thread.to_owned());
}

#[derive(Default)]
struct Log {
    threads: Vec<String>,
    records: Vec<(String, String)>,
}

type Captured = Mutex<Log>;

static LOGGER: OnceLock<&'static Captured> = OnceLock::new();

struct Capture(&'static Captured);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let Some(thread) = std::thread::current().name().map(str::to_owned) else {
            return;
        };
        let mut log = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if log.threads.contains(&thread) {
            log.records.push((thread, record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn install_logger() -> &'static Captured {
    let captured: &'static Captured = Box::leak(Box::default());
    log::set_logger(Box::leak(Box::new(Capture(captured)))).expect("no other logger");
    log::set_max_level(log::LevelFilter::Trace);
    captured
}
