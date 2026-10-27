// SPDX-License-Identifier: LicenseRef-Proprietary

//! Changes no event reports are picked up by re-statting the files. FSEvents'
//! silent losses can't be made on demand, so these tests make a change that
//! gets no event at all: creating a directory the watcher couldn't watch, or
//! any change while [`SOURCE`] stands in for notify's events. None waits on
//! FSEvents.

use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use notify::{Event, EventKind, event::ModifyKind};

use crate::ConfigWatcher;

thread_local! {
    /// What [`ConfigWatcher::new`] reads instead of notify's events, when
    /// set on the thread that calls it.
    pub(crate) static SOURCE: Cell<Option<mpsc::Receiver<notify::Result<Event>>>> =
        const { Cell::new(None) };
}

thread_local! {
    /// What [`ConfigWatcher::new`] reads the time from, when set on the
    /// thread that calls it.
    pub(crate) static CLOCK: Cell<Option<Arc<FakeClock>>> = const { Cell::new(None) };
}

/// An upper bound only, for a loaded host: a re-stat is due every 3 s.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Twice the re-stat period, and a little more.
const QUIET: Duration = Duration::from_secs(7);

/// What a settings store reloading `batch` would see: whether it names
/// `path` and `path` now holds `text`.
fn reloads(batch: &[PathBuf], path: &PathBuf, text: &str) -> bool {
    batch.contains(path) && fs::read_to_string(path).is_ok_and(|now| now == text)
}

#[test]
fn a_file_in_a_directory_made_later_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let path = missing.join("settings.toml");
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(vec![path.clone()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(
        first,
        std::slice::from_ref(&path),
        "the first batch, before any event"
    );

    fs::create_dir(&missing).unwrap();
    fs::write(&path, "editor.font_size = 18\n").unwrap();
    // A re-stat may see the directory before the file is written, and report
    // it then too.
    let deadline = Instant::now() + TIMEOUT;
    let mut reloaded = false;
    while !reloaded {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let batch = batches
            .recv_timeout(remaining)
            .expect("a batch naming the file in a directory made after the watcher started");
        assert_eq!(batch, std::slice::from_ref(&path));
        reloaded = reloads(&batch, &path, "editor.font_size = 18\n");
    }
}

/// A clock that moves on a step with every reading, whatever the thread's
/// scheduling: a deadline passes after a set number of readings.
pub(crate) struct FakeClock {
    base: Instant,
    step: Duration,
    readings: AtomicU32,
}

impl FakeClock {
    pub(crate) fn now(&self) -> Instant {
        self.base + self.step * self.readings.fetch_add(1, Ordering::SeqCst)
    }
}

/// Unrelated events stand in for notify's through [`SOURCE`], all queued
/// before the thread reads any, so it's never without one when the
/// re-stat falls due. [`CLOCK`] makes that due after about 100 readings, of
/// the 10 000 events queued. The watcher never hears of the file's change:
/// only a re-stat finds it, and the backlog mustn't put that off until
/// it's read.
#[test]
fn a_change_amid_unrelated_events_is_reported() {
    const BACKLOG: u32 = 10_000;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, "editor.font_size = 13\n").unwrap();
    let (events, rx) = mpsc::channel();
    for _ in 0..BACKLOG {
        let event =
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path(dir.path().join("unrelated"));
        events.send(Ok(event)).unwrap();
    }
    let clock = Arc::new(FakeClock {
        base: Instant::now(),
        step: crate::watch::RECONCILE / 100,
        readings: AtomicU32::new(0),
    });
    SOURCE.set(Some(rx));
    CLOCK.set(Some(clock.clone()));
    let (tx, batches) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let mut wait = Some(wait);
    let _watcher = ConfigWatcher::new(vec![path.clone()], {
        let clock = clock.clone();
        move |batch| {
            tx.send((batch, clock.readings.load(Ordering::SeqCst))).ok();
            // The first batch holds the thread until the file has changed.
            if let Some(wait) = wait.take() {
                wait.recv_timeout(TIMEOUT).ok();
            }
        }
    })
    .unwrap();
    // Rebound after the watcher, so dropped before it, on a panic too: the
    // thread reads until its sender is gone, and the watcher joins it.
    let _events = events;
    let (first, _) = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(first, std::slice::from_ref(&path));

    fs::write(&path, "editor.font_size = 18\n").unwrap();
    go.send(()).unwrap();
    let (batch, readings) = batches
        .recv_timeout(TIMEOUT)
        .expect("a batch naming the file changed amid unrelated events");
    assert_eq!(batch, std::slice::from_ref(&path));
    assert!(reloads(&batch, &path, "editor.font_size = 18\n"));
    assert!(
        readings < BACKLOG / 2,
        "reported after {readings} readings, once the backlog was read"
    );
}

/// FSEvents may still report the fixture's own writes, made just before the
/// watcher started, so the batches they give are let through. A re-stat that
/// reported the unchanged file would, every 3 s, keep any [`QUIET`] window
/// from passing.
#[test]
fn an_unchanged_file_is_not_reported_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, "editor.font_size = 18\n").unwrap();
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(vec![path.clone()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(first, std::slice::from_ref(&path));
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match batches.recv_timeout(QUIET) {
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Ok(batch) => assert_eq!(batch, std::slice::from_ref(&path)),
            Err(error) => panic!("{error}"),
        }
        assert!(
            Instant::now() < deadline,
            "nothing changed, yet batches kept coming"
        );
    }
}
