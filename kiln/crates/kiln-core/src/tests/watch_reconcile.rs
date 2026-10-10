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
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
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

/// Unrelated events, every 10 ms, stand in for notify's through [`SOURCE`],
/// so the watcher never hears of the file's change: only a re-stat finds it,
/// and the events mustn't put that off.
#[test]
fn a_change_amid_unrelated_events_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, "editor.font_size = 13\n").unwrap();
    let (events, rx) = mpsc::channel();
    SOURCE.set(Some(rx));
    let noise = Noise::start(events, dir.path().join("unrelated"));
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(vec![path.clone()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    // Declared after the watcher, so dropped first: the watcher's thread
    // reads the noise's events until their sender is gone.
    let _noise = noise;
    let first = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(first, std::slice::from_ref(&path));

    fs::write(&path, "editor.font_size = 18\n").unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let mut reloaded = false;
    while !reloaded {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let batch = batches
            .recv_timeout(remaining)
            .expect("a batch naming the file changed amid unrelated events");
        assert_eq!(batch, std::slice::from_ref(&path));
        reloaded = reloads(&batch, &path, "editor.font_size = 18\n");
    }
}

/// Sends an event naming `path`, which no watcher tracks, every 10 ms until
/// dropped; dropping it drops the sender.
struct Noise {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Noise {
    fn start(events: mpsc::Sender<notify::Result<Event>>, path: PathBuf) -> Noise {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::Relaxed) {
                    let event =
                        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.clone());
                    if events.send(Ok(event)).is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });
        Noise {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Noise {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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
