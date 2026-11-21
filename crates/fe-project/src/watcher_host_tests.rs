//! [`LocalWatch`] delivers exactly the batches a [`ProjectWatcher`] hands
//! its sink: both fed the same events through
//! [`crate::tests::watcher_recovery::SOURCE`] rather than the kernel's, whose
//! timing would coalesce them differently.

use std::{
    fs,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use fe_host::WatchHost;
use notify::{
    Event, EventKind,
    event::{CreateKind, Flag, ModifyKind, RemoveKind},
};

use super::{FsChange, LocalWatch, ProjectWatcher};
use crate::tests::watcher_recovery::{SOURCE, Source};

/// An upper bound only, for a loaded machine.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Long enough that no rescan is due while a test runs.
const SETTLE: [Duration; 3] = [Duration::from_secs(600); 3];

/// Creates, changes and removals under `root`, a folder, and a flagged
/// rescan: what the watcher reads instead of the kernel's
/// events. The receiver ends once they're read.
fn events(root: &Path) -> mpsc::Receiver<notify::Result<Event>> {
    fs::create_dir(root.join("made")).unwrap();
    fs::write(root.join("a.rs"), "").unwrap();
    let (tx, rx) = mpsc::channel();
    let event = |kind, rel: &str| Ok(Event::new(kind).add_path(root.join(rel)));
    for event in [
        event(EventKind::Create(CreateKind::File), "a.rs"),
        event(EventKind::Modify(ModifyKind::Any), "a.rs"),
        event(EventKind::Create(CreateKind::Folder), "made"),
        event(EventKind::Remove(RemoveKind::File), "gone.rs"),
        Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan)),
    ] {
        tx.send(event).unwrap();
    }
    rx
}

/// Sets what the next watcher made on this thread reads.
fn feed(root: &Path) {
    SOURCE.set(Some(Source {
        events: events(root),
        settle: SETTLE,
    }));
}

/// Every batch `next` gives until its sender is gone.
fn every_batch(
    mut next: impl FnMut(Duration) -> Result<Vec<FsChange>, mpsc::RecvTimeoutError>,
) -> Vec<Vec<FsChange>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut batches = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match next(left) {
            Ok(batch) => batches.push(batch),
            Err(mpsc::RecvTimeoutError::Disconnected) => return batches,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("the watcher didn't end"),
        }
    }
}

#[test]
fn a_hosted_watch_delivers_the_watchers_batches_exactly() {
    let direct_dir = tempfile::tempdir().unwrap();
    let direct_root = dunce::canonicalize(direct_dir.path()).unwrap();
    feed(&direct_root);
    let (tx, rx) = mpsc::channel();
    let _watcher = ProjectWatcher::new(&direct_root, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();
    let direct = every_batch(|left| rx.recv_timeout(left));
    let path = |rel: &str| FsChange::Path(direct_root.join(rel));
    assert_eq!(
        direct,
        [vec![
            path("a.rs"),
            path("made"),
            path("gone.rs"),
            FsChange::Rescan
        ]]
    );

    let hosted_dir = tempfile::tempdir().unwrap();
    let hosted_root = dunce::canonicalize(hosted_dir.path()).unwrap();
    feed(&hosted_root);
    let host: &dyn WatchHost = &LocalWatch;
    let subscription = host.watch(&hosted_root).unwrap();
    let hosted = every_batch(|left| subscription.recv_timeout(left));

    // The same fixture in another folder: the same batches, rooted there.
    let rerooted: Vec<Vec<FsChange>> = (direct.into_iter())
        .map(|batch| {
            (batch.into_iter())
                .map(|change| match change {
                    FsChange::Path(path) => {
                        FsChange::Path(hosted_root.join(path.strip_prefix(&direct_root).unwrap()))
                    }
                    FsChange::Rescan => FsChange::Rescan,
                })
                .collect()
        })
        .collect();
    assert_eq!(hosted, rerooted);
}
