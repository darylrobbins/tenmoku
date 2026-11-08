//! The Linux watcher's own inotify watches: which directories get them, at
//! the limit through [`CAP`], and those created later.

use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, event::Flag};
use tempfile::TempDir;

use super::*;

thread_local! {
    /// How many watches [`ProjectWatcher::new`] may make, when set on the
    /// thread that calls it, standing in for `fs.inotify.max_user_watches`.
    pub(crate) static CAP: Cell<Option<usize>> = const { Cell::new(None) };
}

thread_local! {
    /// Called on the thread that calls [`ProjectWatcher::new`] with each
    /// directory it has opened, before watching it.
    static OPENED: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

type Hook = Box<dyn FnMut(&Path)>;

/// Maps each event the watcher's thread reads, standing in for losses.
pub(crate) type Tap = Box<dyn FnMut(notify::Result<Event>) -> notify::Result<Event> + Send>;

thread_local! {
    /// What [`ProjectWatcher::new`] does to the events its thread reads,
    /// when set on the thread that calls it.
    pub(crate) static TAP: Cell<Option<Tap>> = const { Cell::new(None) };
}

/// See [`OPENED`].
pub(crate) fn opened(dir: &Path) {
    OPENED.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook(dir);
        }
    });
}

/// An upper bound only, for a loaded host: inotify delivers in order and
/// doesn't drop events short of a queue overflow.
const TIMEOUT: Duration = Duration::from_secs(10);

/// How long nothing more may arrive.
const QUIET: Duration = Duration::from_millis(300);

/// A watcher over a fresh directory holding `paths`: a trailing `/` makes a
/// directory, anything else a file with `contents` as its contents.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    _watcher: ProjectWatcher,
    batches: mpsc::Receiver<Vec<FsChange>>,
    /// Every change received, in order.
    received: Vec<FsChange>,
}

impl Project {
    fn new(paths: &[(&str, &str)], cap: Option<usize>) -> Project {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        make(&root, paths);
        Project::watch(dir, root, cap)
    }

    /// Watches `root`, the canonical path of `dir`.
    fn watch(dir: TempDir, root: PathBuf, cap: Option<usize>) -> Project {
        let (tx, batches) = mpsc::channel();
        CAP.set(cap);
        let watcher = ProjectWatcher::new(&root, move |batch| {
            tx.send(batch).ok();
        })
        .unwrap();
        Project {
            _dir: dir,
            root,
            _watcher: watcher,
            batches,
            received: Vec::new(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str) {
        fs::write(self.path(rel), "changed").unwrap();
    }

    /// Waits until a batch names `rel`.
    fn wait_for(&mut self, rel: &str) {
        let want = FsChange::Path(self.path(rel));
        let deadline = Instant::now() + TIMEOUT;
        while !self.received.contains(&want) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.batches.recv_timeout(left) {
                Ok(batch) => self.received.extend(batch),
                Err(_) => panic!("{rel} not delivered; received {:?}", self.received),
            }
        }
    }

    /// Takes in whatever else arrives until [`QUIET`] passes with nothing.
    fn settle(&mut self) {
        while let Ok(batch) = self.batches.recv_timeout(QUIET) {
            self.received.extend(batch);
        }
    }

    /// The changes received naming `rel` or a path under it.
    fn under(&self, rel: &str) -> Vec<&FsChange> {
        let dir = self.path(rel);
        self.received
            .iter()
            .filter(|change| matches!(change, FsChange::Path(path) if path.starts_with(&dir)))
            .collect()
    }
}

fn make(root: &Path, paths: &[(&str, &str)]) {
    for (path, contents) in paths {
        let full = root.join(path);
        if path.ends_with('/') {
            fs::create_dir_all(full).unwrap();
        } else {
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
    }
}

#[test]
fn ignored_directories_go_last() {
    let paths = [
        (".gitignore", "target/\n"),
        ("src/a/", ""),
        ("src/b/", ""),
        ("target/x/", ""),
    ];
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    make(&root, &paths);
    let listed = listed(&root);
    // The root, src, src/a and src/b.
    assert_eq!(listed, 4);
    let mut project = Project::watch(dir, root, Some(listed));

    project.write("src/b/f");
    project.wait_for("src/b/f");
    // inotify delivers in order: had target/x been watched, its write would
    // come before the one after it.
    project.write("target/x/f");
    project.write("src/b/g");
    project.wait_for("src/b/g");
    project.settle();

    assert_eq!(project.under("target"), Vec::<&FsChange>::new());
    assert_eq!(rescans(&project), 1, "received {:?}", project.received);
}

/// The directories `walk_builder` lists under `root`, the root included.
fn listed(root: &Path) -> usize {
    crate::listing::walk_builder(root, |_| true)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_dir()))
        .count()
}

fn rescans(project: &Project) -> usize {
    project
        .received
        .iter()
        .filter(|change| **change == FsChange::Rescan)
        .count()
}

#[test]
fn ignored_directories_are_watched_by_depth() {
    let paths = [
        (".gitignore", "target/\ncache/\n"),
        ("src/a/b/cache/", ""),
        ("target/x/", ""),
    ];
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    make(&root, &paths);
    // The root, src, src/a and src/b.
    assert_eq!(listed(&root), 4);
    // Then target and target/x, at depths 1 and 2, before src/a/b/cache at 4.
    let mut project = Project::watch(dir, root, Some(6));

    project.write("src/a/b/cache/f");
    project.write("target/x/f");
    project.wait_for("target/x/f");
    project.settle();

    assert_eq!(project.under("src/a/b/cache"), Vec::<&FsChange>::new());
    assert_eq!(rescans(&project), 1, "received {:?}", project.received);
}

#[test]
fn a_path_through_a_symlink_is_not_watched() {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let root = base.join("project");
    make(&root, &[("src/a/sub/", ""), ("src/main.rs", "")]);
    make(&base, &[("outside/src/a/sub/", "")]);
    // Once src/a is opened, before its watch, the project moves aside and a
    // symlink to a directory outside it takes its place, where no watched
    // directory sees it: inotify would follow it to outside/src/a.
    let (from, moved, outside) = (root.join("src/a"), base.join("moved"), base.join("outside"));
    let swap = {
        let root = root.clone();
        move |dir: &Path| {
            if dir == from && !moved.exists() {
                fs::rename(&root, &moved).unwrap();
                std::os::unix::fs::symlink(&outside, &root).unwrap();
            }
        }
    };
    OPENED.set(Some(Box::new(swap)));
    let mut project = Project::watch(dir, root, None);
    OPENED.set(None);
    assert!(base.join("moved").is_dir());
    project.settle();
    project.received.clear();

    fs::write(base.join("outside/src/a/f"), "").unwrap();
    fs::write(base.join("outside/src/a/sub/f"), "").unwrap();
    // Its watch, made before the swap, names it as it was.
    fs::write(base.join("moved/src/main.rs"), "changed").unwrap();
    project.wait_for("src/main.rs");
    project.settle();

    assert_eq!(project.under("src/a"), Vec::<&FsChange>::new());
}

#[test]
fn new_directories_are_watched() {
    let mut project = Project::new(&[("src/", "")], None);

    fs::create_dir_all(project.path("new/a/b")).unwrap();
    project.write("new/a/b/f");
    project.wait_for("new/a/b/f");

    project.write("new/a/b/g");
    project.wait_for("new/a/b/g");
    project.settle();
    assert!(
        !project.received.contains(&FsChange::Rescan),
        "received {:?}",
        project.received
    );
}

#[test]
fn git_objects_are_not_watched() {
    let mut project = Project::new(
        &[
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".git/objects/ab/", ""),
            ("src/main.rs", ""),
        ],
        None,
    );

    // inotify delivers in order: had either been watched, its write would
    // come before HEAD's.
    project.write(".git/objects/pack-file");
    project.write(".git/objects/ab/cd");
    project.write(".git/HEAD");
    project.wait_for(".git/HEAD");
    project.settle();

    let head = FsChange::Path(project.path(".git/HEAD"));
    assert!(
        project.received.iter().all(|change| *change == head),
        "received {:?}",
        project.received
    );
}

#[test]
fn a_symlink_to_a_watched_directory_leaves_its_watch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    make(&root, &[("a/", ""), ("src/b/", "")]);
    // Once src/b is opened, before its watch, it moves aside and a symlink to
    // a, watched already (it is shallower), takes its place: inotify shares
    // a's watch with it.
    let (b, moved, a) = (root.join("src/b"), root.join("moved"), root.join("a"));
    OPENED.set(Some(Box::new(move |dir: &Path| {
        if dir == b && !moved.exists() {
            fs::rename(&b, &moved).unwrap();
            std::os::unix::fs::symlink(&a, &b).unwrap();
        }
    })));
    let mut project = Project::watch(dir, root, None);
    OPENED.set(None);
    assert!(project.path("moved").is_dir());
    project.settle();
    project.received.clear();

    project.write("a/f");
    project.wait_for("a/f");
    project.settle();
    assert_eq!(project.under("src/b"), Vec::<&FsChange>::new());
}

#[test]
fn new_directories_put_ignored_ones_last() {
    let base = tempfile::tempdir().unwrap();
    let base_path = dunce::canonicalize(base.path()).unwrap();
    let dir = tempfile::tempdir_in(&base_path).unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    make(&root, &[(".gitignore", "out/\n")]);
    // The root alone.
    assert_eq!(listed(&root), 1);
    // Then new and new/src, before new/out.
    let mut project = Project::watch(dir, root, Some(3));
    // Moved in whole, so the walk finds all of it at once.
    make(&base_path, &[("staging/src/", ""), ("staging/out/", "")]);
    fs::rename(base_path.join("staging"), project.path("new")).unwrap();
    project.wait_for("new/out");

    project.write("new/out/f");
    project.write("new/src/f");
    project.wait_for("new/src/f");
    project.settle();

    assert_eq!(project.under("new/out/f"), Vec::<&FsChange>::new());
    assert_eq!(rescans(&project), 1, "received {:?}", project.received);
}

#[test]
fn a_directory_whose_event_is_lost_is_watched_after_a_rescan() {
    let base = tempfile::tempdir().unwrap();
    let base_path = dunce::canonicalize(base.path()).unwrap();
    let dir = tempfile::tempdir_in(&base_path).unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    make(&root, &[("src/", "")]);
    // The events for new become an overflow's.
    let new = root.join("new");
    TAP.set(Some(Box::new(move |event| match event {
        Ok(event) if event.paths.contains(&new) => {
            Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan))
        }
        event => event,
    })));
    let mut project = Project::watch(dir, root, None);
    make(&base_path, &[("staging/a/", "")]);
    fs::rename(base_path.join("staging"), project.path("new")).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while rescans(&project) == 0 {
        let left = deadline.saturating_duration_since(Instant::now());
        let batch = project.batches.recv_timeout(left).expect("a rescan");
        project.received.extend(batch);
    }

    project.write("new/a/f");
    project.wait_for("new/a/f");
}
