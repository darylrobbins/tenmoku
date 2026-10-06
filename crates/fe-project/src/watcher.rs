//! Watching the project directory for changes.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
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
/// Dropping the watcher stops it and waits for the thread.
pub struct ProjectWatcher {
    watcher: Option<RecommendedWatcher>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ProjectWatcher {
    pub fn new(
        root: impl AsRef<Path>,
        mut sink: impl FnMut(Vec<FsChange>) + Send + 'static,
    ) -> notify::Result<ProjectWatcher> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;
        let thread = thread::Builder::new()
            .name("project-watcher".into())
            .spawn(move || {
                // Ends when the watcher, which owns the sender, is dropped.
                while let Ok(first) = rx.recv() {
                    let mut batch = Batch::default();
                    batch.add(first);
                    while let Ok(next) = rx.try_recv() {
                        batch.add(next);
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
