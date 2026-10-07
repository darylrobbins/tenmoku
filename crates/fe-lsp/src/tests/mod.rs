//! Unit tests, one module per area. They run servers through the fake in
//! `crate::fake` (declared in lib.rs, so fe-workbench can share it).

mod launcher;
mod lifecycle;
mod router;

use std::future::Future;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use futures::FutureExt;

use crate::{Clock, LspEvent, LspManager, ServerId, ServerLauncher, ServerStatus, SystemClock};

/// How long a test waits for anything before failing.
const WAIT: Duration = Duration::from_secs(10);

/// A manager whose events land in the returned [`Events`].
fn manager(
    launcher: Arc<dyn ServerLauncher>,
    clock: Option<Arc<dyn Clock>>,
) -> (LspManager, Events) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let sink = move |event| {
        let _ = sender.send(event);
    };
    let manager = match clock {
        Some(clock) => LspManager::with_clock(launcher, clock, sink),
        None => LspManager::new(launcher, sink),
    };
    (manager, Events(receiver))
}

struct Events(Receiver<LspEvent>);

impl Events {
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

    /// Asserts nothing arrives for a moment of real time.
    fn assert_quiet(&self) {
        if let Ok(event) = self.0.recv_timeout(Duration::from_millis(100)) {
            panic!("unexpected {event:?}");
        }
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
