//! Watching the project directory for changes.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Something that may have changed on disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FsChange {
    /// Something at this path may have been created, changed or removed.
    Path(PathBuf),
    /// Events were dropped or coalesced; anything may have changed.
    Rescan,
}

/// Watches a directory recursively (FSEvents on macOS) and hands batches of
/// [`FsChange`]s to a sink on its own thread. A batch is every event queued
/// when the thread wakes, with each path once.
///
/// FSEvents can drop events without flagging it while something writes many
/// files, such as a `git checkout`, a build or `npm install`: delivery stops
/// for tens of seconds, then resumes. So after a burst, [`BURST`] distinct
/// paths delivered with no [`SETTLE`]`[0]` of quiet between them, the
/// watcher also sends `[FsChange::Rescan]` once the stream has been quiet for
/// each of [`SETTLE`]'s periods, measured from the last event received. A
/// loss window looks like quiet, so the first rescan may land mid-flood;
/// the last covers a loss that outlasts the last delivered event by up to a
/// minute. Known limits: a flood outside the root that drops this root's
/// events isn't detected, and neither is a loss window more than
/// [`SETTLE`]`[2]` past the last delivered event.
///
/// Dropping the watcher stops it and waits for the thread.
pub struct ProjectWatcher {
    watcher: Option<RecommendedWatcher>,
    thread: Option<thread::JoinHandle<()>>,
}

/// Distinct paths that make a burst: see [`ProjectWatcher`].
const BURST: usize = 1_000;

/// How long the stream must be quiet, after a burst, for each rescan: see
/// [`ProjectWatcher`]. The first also ends the count towards a burst.
const SETTLE: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

type Events = mpsc::Receiver<notify::Result<Event>>;

impl ProjectWatcher {
    pub fn new(
        root: impl AsRef<Path>,
        mut sink: impl FnMut(Vec<FsChange>) + Send + 'static,
    ) -> notify::Result<ProjectWatcher> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;
        let (rx, settle) = source(rx);
        let thread = thread::Builder::new()
            .name("project-watcher".into())
            .spawn(move || {
                let mut recovery = Recovery::new(settle);
                loop {
                    let first = match recovery.due() {
                        // Ends when the watcher, which owns the sender, is
                        // dropped.
                        None => match rx.recv() {
                            Ok(first) => first,
                            Err(mpsc::RecvError) => break,
                        },
                        Some(due) => {
                            match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                                Ok(first) => first,
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    recovery.rescanned();
                                    sink(vec![FsChange::Rescan]);
                                    continue;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    };
                    let mut events = vec![first];
                    events.extend(rx.try_iter());
                    recovery.received(&events, Instant::now());
                    let mut batch = Batch::default();
                    for event in events {
                        batch.add(event);
                    }
                    if !batch.changes.is_empty() {
                        sink(batch.changes);
                    }
                }
            })?;
        Ok(ProjectWatcher {
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl Drop for ProjectWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The events the thread reads, and its [`SETTLE`] periods: in tests,
/// [`crate::tests::watcher_recovery::SOURCE`] stands in, if set on this
/// thread.
#[cfg(not(test))]
fn source(events: Events) -> (Events, [Duration; 3]) {
    (events, SETTLE)
}

#[cfg(test)]
fn source(events: Events) -> (Events, [Duration; 3]) {
    match crate::tests::watcher_recovery::SOURCE.take() {
        Some(source) => (source.events, source.settle),
        None => (events, SETTLE),
    }
}

/// When to rescan after a burst: see [`ProjectWatcher`].
struct Recovery {
    settle: [Duration; 3],
    /// When the last event was received.
    last: Option<Instant>,
    /// Distinct paths delivered since the stream was last quiet for
    /// `settle[0]`, while no burst is pending.
    paths: HashSet<PathBuf>,
    /// After a burst, how many of its rescans were sent.
    rescans: Option<usize>,
}

impl Recovery {
    fn new(settle: [Duration; 3]) -> Self {
        Self {
            settle,
            last: None,
            paths: HashSet::new(),
            rescans: None,
        }
    }

    /// When the next rescan is due, if a burst is pending.
    fn due(&self) -> Option<Instant> {
        let last = self.last?;
        Some(last + self.settle[self.rescans?])
    }

    /// Counts the paths `events`, received `now`, name towards a burst, and
    /// restarts the quiet clock. Every path counts, including those of events
    /// a [`Batch`] turns into [`FsChange::Rescan`], but not unflagged
    /// [`EventKind::Access`] ones.
    fn received(&mut self, events: &[notify::Result<Event>], now: Instant) {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) >= self.settle[0])
        {
            self.paths.clear();
        }
        self.last = Some(now);
        if self.rescans.is_some() {
            return;
        }
        for event in events.iter().flatten() {
            // Reads don't change the tree, and a rescan's own opens would
            // re-arm it; but a flagged event's paths still count.
            if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan() {
                continue;
            }
            self.paths.extend(event.paths.iter().cloned());
        }
        if self.paths.len() >= BURST {
            self.rescans = Some(0);
            self.paths = HashSet::new();
        }
    }

    /// Notes that the rescan [`Recovery::due`] was sent.
    fn rescanned(&mut self) {
        self.rescans = self
            .rescans
            .map(|sent| sent + 1)
            .filter(|&sent| sent < self.settle.len());
    }
}

#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) changes: Vec<FsChange>,
    seen: HashSet<FsChange>,
}

impl Batch {
    pub(crate) fn add(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(event) if event.need_rescan() => self.push(FsChange::Rescan),
            // Reads don't change the tree.
            Ok(Event {
                kind: EventKind::Access(_),
                ..
            }) => {}
            Ok(event) => {
                for path in event.paths {
                    self.push(FsChange::Path(path));
                }
            }
            Err(_) => self.push(FsChange::Rescan),
        }
    }

    fn push(&mut self, change: FsChange) {
        if self.seen.insert(change.clone()) {
            self.changes.push(change);
        }
    }
}
