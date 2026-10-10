//! The watcher test waits on real FSEvents delivery, which under load isn't
//! bounded: fseventsd is one daemon shared by every process. Each wait runs
//! beside a [`Control`], a raw stream over a directory of its own, so a
//! timeout fails only when FSEvents demonstrably delivered past the change.
//! That holds only while nothing floods FSEvents, which drops events silently
//! then: the `fsevents` test group in `.config/nextest.toml` keeps this test
//! apart from the 100k-file ones, and as a flood's backlog outlives its test,
//! each change waits until the control shows FSEvents delivering again.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, Flag, ModifyKind},
};
use tempfile::TempDir;

use super::*;
use crate::watcher::Batch;

/// How long a wait lasts, with or without delivery.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long before the deadline the control must have seen its sentinel for
/// a timeout to count as the watcher's failure.
const MARGIN: Duration = Duration::from_secs(10);

/// The control's readiness check: [`READY_RUN`] sentinels in a row, written
/// [`READY_GAP`] apart, all reported.
const READY_RUN: usize = 5;
const READY_GAP: Duration = Duration::from_millis(100);

/// A raw notify stream over a sentinel directory outside every watched
/// path, showing how far FSEvents has delivered.
struct Control {
    _dir: TempDir,
    root: PathBuf,
    marks: usize,
    _watcher: RecommendedWatcher,
    events: mpsc::Receiver<(Instant, notify::Result<Event>)>,
    /// When each reported path was first reported.
    seen: HashMap<PathBuf, Instant>,
}

impl Control {
    /// Starts the stream; it's running on return.
    fn new() -> Control {
        let (dir, root) = temp();
        let (tx, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            tx.send((Instant::now(), event)).ok();
        })
        .unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        Control {
            _dir: dir,
            root,
            marks: 0,
            _watcher: watcher,
            events,
            seen: HashMap::new(),
        }
    }

    /// Writes a new sentinel file and returns its path.
    fn mark(&mut self) -> PathBuf {
        self.marks += 1;
        let path = self.root.join(format!("sentinel-{}", self.marks));
        fs::write(&path, "").unwrap();
        path
    }

    /// When the stream first reported `path`, if it has.
    fn seen(&mut self, path: &Path) -> Option<Instant> {
        for (at, event) in self.events.try_iter() {
            for path in event.into_iter().flat_map(|event| event.paths) {
                self.seen.entry(path).or_insert(at);
            }
        }
        self.seen.get(path).copied()
    }

    /// Waits, up to [`TIMEOUT`], until FSEvents is delivering: [`READY_RUN`]
    /// sentinels in a row reported. A flood's backlog outlives the test that
    /// made it, and until it drains FSEvents drops events silently, so a
    /// change made then could be lost while a later sentinel still arrives.
    /// False, noting it, if FSEvents doesn't catch up in time.
    fn ready(&mut self, what: &str) -> bool {
        let start = Instant::now();
        let mut marks = Vec::new();
        while start.elapsed() < TIMEOUT {
            marks.push(self.mark());
            thread::sleep(READY_GAP);
            let seen: Vec<bool> = marks.iter().map(|mark| self.seen(mark).is_some()).collect();
            if seen
                .windows(READY_RUN)
                .any(|run| run.iter().all(|&seen| seen))
            {
                if start.elapsed() > Duration::from_secs(2) {
                    eprintln!(
                        "FSEvents caught up before {what} after {:?}",
                        start.elapsed()
                    );
                }
                return true;
            }
        }
        eprintln!(
            "FSEvents delivered nothing past the control's sentinels before {what} in 60 s; \
             not a watcher failure"
        );
        false
    }

    /// At a wait's `deadline`, fails if the control saw any of `marks`, the
    /// sentinels written after the change, at least [`MARGIN`] before it;
    /// otherwise notes that FSEvents delivered nothing past `what`.
    fn timed_out(&mut self, marks: &[PathBuf], deadline: Instant, what: &str) {
        let delivered = marks
            .iter()
            .filter_map(|mark| self.seen(mark))
            .any(|at| at + MARGIN <= deadline);
        assert!(
            !delivered,
            "no batch showing {what} within {TIMEOUT:?}, though FSEvents delivered past it",
        );
        eprintln!("FSEvents delivered nothing past {what} in 60 s; not a watcher failure");
    }
}

/// Once `control` is [ready](Control::ready), makes `change`, marks it, and
/// applies batches from `rx` until `done` holds for the tree, checking that
/// no batch repeats a path. Waits up to [`TIMEOUT`]; how long the events take
/// isn't asserted. If the deadline passes, the test fails only if the control
/// saw its sentinel at least [`MARGIN`] before it; otherwise FSEvents
/// delivered nothing past the change, which is noted. Then, or if the control
/// never got ready, this returns false for the test to end early, passing.
/// The ci profile's `retries = 2` override in `.config/nextest.toml` stays.
#[must_use]
fn wait_for(
    tree: &mut FileTree,
    rx: &mpsc::Receiver<Vec<FsChange>>,
    control: &mut Control,
    what: &str,
    change: impl FnOnce(),
    done: impl Fn(&FileTree) -> bool,
) -> bool {
    if !control.ready(what) {
        return false;
    }
    change();
    let marks = [control.mark()];
    let deadline = Instant::now() + TIMEOUT;
    while !done(tree) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            control.timed_out(&marks, deadline, what);
            return false;
        }
        let batch = match rx.recv_timeout(left) {
            Ok(batch) => batch,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(err) => panic!("no batch showing {what}: {err}"),
        };
        let unique: HashSet<_> = batch.iter().collect();
        assert_eq!(
            unique.len(),
            batch.len(),
            "a batch repeats a path: {batch:?}"
        );
        tree.apply(&batch);
    }
    true
}

#[test]
fn watcher_batches_create_rename_delete() {
    let (_dir, root) = temp();

    // Within a batch each path, and the rescan marker, appears once. First,
    // as the waits below may end the test early.
    let event = |kind, path: &str| Ok(Event::new(kind).add_path(root.join(path)));
    let mut batch = Batch::default();
    batch.add(event(EventKind::Create(CreateKind::File), "a"));
    batch.add(event(EventKind::Modify(ModifyKind::Any), "a"));
    batch.add(event(EventKind::Create(CreateKind::File), "b"));
    batch.add(Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan)));
    batch.add(Err(notify::Error::generic("dropped")));
    batch.add(event(EventKind::Modify(ModifyKind::Any), "b"));
    assert_eq!(
        batch.changes,
        [
            FsChange::Path(root.join("a")),
            FsChange::Path(root.join("b")),
            FsChange::Rescan,
        ]
    );

    make(&root, &["src/main.rs"]);
    let mut tree = FileTree::open(&root, TreeOptions::default()).unwrap();
    tree.expand(id(&tree, "src"));
    let mut control = Control::new();
    let (tx, rx) = mpsc::channel();
    let _watcher = ProjectWatcher::new(&root, move |batch| {
        let _ = tx.send(batch);
    })
    .unwrap();

    let create = || fs::write(root.join("src/new.rs"), "fn main() {}").unwrap();
    let created = |tree: &FileTree| tree.find("src/new.rs").is_some();
    if !wait_for(
        &mut tree,
        &rx,
        &mut control,
        "the created file",
        create,
        created,
    ) {
        return;
    }
    assert_eq!(layout(&tree), ["v src", "  main.rs", "  new.rs"]);

    let rename = || fs::rename(root.join("src/new.rs"), root.join("src/renamed.rs")).unwrap();
    let renamed = |tree: &FileTree| {
        tree.find("src/new.rs").is_none() && tree.find("src/renamed.rs").is_some()
    };
    if !wait_for(&mut tree, &rx, &mut control, "the rename", rename, renamed) {
        return;
    }
    assert_eq!(layout(&tree), ["v src", "  main.rs", "  renamed.rs"]);

    let delete = || fs::remove_file(root.join("src/renamed.rs")).unwrap();
    let deleted = |tree: &FileTree| tree.find("src/renamed.rs").is_none();
    if !wait_for(
        &mut tree,
        &rx,
        &mut control,
        "the deletion",
        delete,
        deleted,
    ) {
        return;
    }
    assert_eq!(layout(&tree), ["v src", "  main.rs"]);
}
