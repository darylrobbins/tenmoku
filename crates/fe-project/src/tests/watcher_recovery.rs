//! Rescans after a burst, fed synthetic events through [`SOURCE`] rather
//! than FSEvents, whose silent losses can't be made on demand.

use std::{
    cell::Cell,
    collections::HashSet,
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind,
    event::{AccessKind, AccessMode, CreateKind, Flag},
};

use super::*;

/// What [`ProjectWatcher::new`] reads instead of notify's events, with
/// these settle periods, when set on the thread that calls it.
pub(crate) struct Source {
    pub(crate) events: mpsc::Receiver<notify::Result<notify::Event>>,
    pub(crate) settle: [Duration; 3],
}

thread_local! {
    pub(crate) static SOURCE: Cell<Option<Source>> = const { Cell::new(None) };
}

const SETTLE: [Duration; 3] = [
    Duration::from_millis(200),
    Duration::from_millis(600),
    Duration::from_secs(1),
];

/// An upper bound only, for a loaded host.
const TIMEOUT: Duration = Duration::from_secs(10);

/// How long nothing more may arrive.
const QUIET: Duration = Duration::from_secs(2);

/// A watcher fed synthetic events. Dropping it drops their sender before the
/// watcher, whose thread reads them until the sender is gone.
struct Burst {
    events: Option<mpsc::Sender<notify::Result<Event>>>,
    _watcher: ProjectWatcher,
    _dir: TempDir,
    batches: mpsc::Receiver<Vec<FsChange>>,
    /// When the last event was sent, before the watcher started.
    sent: Instant,
}

impl Drop for Burst {
    fn drop(&mut self) {
        drop(self.events.take());
    }
}

/// A watcher over a fresh directory, fed `count` distinct `Create` events,
/// the last flagged for a rescan if `flagged`, once every change they make
/// has arrived in batches naming only those, each once. The events are all
/// queued before the watcher starts, so it reads them without a pause.
fn burst(count: usize, flagged: bool) -> Burst {
    let (dir, root) = temp();
    let (events, rx) = mpsc::channel();
    let mut expected = HashSet::new();
    for index in 0..count {
        let path = root.join(format!("file-{index}"));
        let mut event = Event::new(EventKind::Create(CreateKind::File)).add_path(path.clone());
        if flagged && index == count - 1 {
            event = event.set_flag(Flag::Rescan);
            expected.insert(FsChange::Rescan);
        } else {
            expected.insert(FsChange::Path(path));
        }
        events.send(Ok(event)).unwrap();
    }
    let sent = Instant::now();
    SOURCE.set(Some(Source {
        events: rx,
        settle: SETTLE,
    }));
    let (tx, batches) = mpsc::channel();
    let watcher = ProjectWatcher::new(&root, move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let burst = Burst {
        events: Some(events),
        _watcher: watcher,
        _dir: dir,
        batches,
        sent,
    };
    let deadline = Instant::now() + TIMEOUT;
    while !expected.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let batch = burst
            .batches
            .recv_timeout(remaining)
            .expect("every change in a batch in time");
        for change in batch {
            assert!(expected.remove(&change), "unexpected {change:?}");
        }
    }
    burst
}

/// After the burst's own changes, three `[Rescan]` batches, each once the
/// stream has been quiet for its settle period, and nothing more.
fn assert_rescans(burst: &Burst) {
    for settle in SETTLE {
        let batch = burst
            .batches
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|error| panic!("a rescan {settle:?} after the burst: {error}"));
        assert_eq!(batch, [FsChange::Rescan]);
        assert!(
            burst.sent.elapsed() >= settle,
            "a rescan before the stream was quiet for {settle:?}"
        );
    }
    match burst.batches.recv_timeout(QUIET) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("only three rescans, yet: {other:?}"),
    }
}

#[test]
fn a_burst_is_followed_by_rescans() {
    assert_rescans(&burst(1_000, false));
}

/// A [`Batch`] turns a flagged event into [`FsChange::Rescan`], but its path
/// still counts towards the burst.
#[test]
fn a_rescan_events_path_counts_towards_a_burst() {
    assert_rescans(&burst(1_000, true));
}

#[test]
fn a_small_batch_is_not_followed_by_a_rescan() {
    let burst = burst(999, false);
    match burst.batches.recv_timeout(QUIET) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("no burst, yet: {other:?}"),
    }
}

/// Reads don't change the tree, and on Linux a rescan's own directory opens
/// are reported: they mustn't arm another.
#[test]
fn a_burst_of_reads_is_not_followed_by_a_rescan() {
    let (dir, root) = temp();
    let (events, rx) = mpsc::channel();
    for index in 0..2_000 {
        let event = Event::new(EventKind::Access(AccessKind::Open(AccessMode::Any)))
            .add_path(root.join(format!("dir-{index}")));
        events.send(Ok(event)).unwrap();
    }
    SOURCE.set(Some(Source {
        events: rx,
        settle: SETTLE,
    }));
    let (tx, batches) = mpsc::channel();
    let _watcher = ProjectWatcher::new(&root, move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    match batches.recv_timeout(QUIET) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        other => panic!("only reads, yet: {other:?}"),
    }
    drop(events);
    drop(dir);
}
