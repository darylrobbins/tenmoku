//! The real-watcher tests are the only ones that wait on the file system.
//! Each change gets a fresh watcher, started after its fixture is loaded, so
//! no batch from an earlier change can stand in for it. Waits are bounded by
//! [`TIMEOUT`], and no duration is asserted. FSEvents delivery under load
//! isn't bounded (fseventsd is one daemon shared by every process), so each
//! wait for an event runs beside a [`Control`], a raw stream over a directory
//! of its own: a timeout fails only if the control saw a sentinel written
//! after the change at least [`MARGIN`] before the deadline, and otherwise is
//! noted and passes. That holds only while nothing floods FSEvents, which
//! drops events silently then: the `fsevents` test group in
//! `.config/nextest.toml` keeps these tests apart from the 100k-file ones,
//! and as a flood's backlog outlives its test, each change waits until the
//! control shows FSEvents delivering again.
//! The first batches, built from the files rather than from events, wait as
//! before. The watcher's own tests, symlink tracking included, are
//! kiln-core's.

use std::{
    collections::HashMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tempfile::TempDir;

use crate::{ConfigWatcher, SettingsStore};

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long before the deadline the control must have seen a sentinel for a
/// timeout to count as the watcher's failure.
const MARGIN: Duration = Duration::from_secs(10);

/// The control's readiness check: [`READY_RUN`] sentinels in a row, written
/// [`READY_GAP`] apart, all reported.
const READY_RUN: usize = 5;
const READY_GAP: Duration = Duration::from_millis(100);

/// A twin of kiln-core's own (its watch tests), kept apart so neither
/// crate needs the other's test code.
///
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
        let dir = tempfile::tempdir().unwrap();
        // Canonical, as events name paths.
        let root = dir.path().canonicalize().unwrap();
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

/// Loads `path`, starts a fresh watcher over it, takes its first batch, then
/// makes `change` and reloads each batch naming `path` until `done` holds.
/// One save can arrive in several batches, the first maybe before the write
/// finishes. The change waits until the control is [ready](Control::ready).
/// `None` if the control never got ready or FSEvents delivered nothing past
/// the change in time: see [`Control`].
fn after_change(
    path: &Path,
    change: impl FnOnce(),
    done: impl Fn(&SettingsStore) -> bool,
) -> Option<SettingsStore> {
    let mut store = SettingsStore::load(Some(path.to_owned()), None);
    let mut control = Control::new();
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(vec![path.to_owned()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(first, [path], "the first batch, before any event");
    if !control.ready("the change") {
        return None;
    }
    change();
    let marks = [control.mark()];
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            control.timed_out(&marks, deadline, "the change");
            return None;
        }
        let batch = match batches.recv_timeout(remaining) {
            Ok(batch) => batch,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => panic!("a batch in time: {error}"),
        };
        assert_eq!(batch, [path]);
        store.reload(&batch);
        if done(&store) {
            return Some(store);
        }
    }
}

fn font_size(size: f64) -> impl Fn(&SettingsStore) -> bool {
    move |store| store.settings().editor.font_size == size
}

fn has_error(store: &SettingsStore) -> bool {
    store.diagnostics().iter().any(|d| d.is_error())
}

/// `dir/settings.toml`, holding `text`.
fn settings_in(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("settings.toml");
    fs::write(&path, text).unwrap();
    path
}

/// Writes `text` to a temporary file beside `path` and renames it over it.
fn replace(path: &Path, text: &str) {
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, text).unwrap();
    fs::rename(&temp, path).unwrap();
}

#[test]
fn hot_reload_applies_valid_and_keeps_last_good() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_in(dir.path(), "editor.font_size = 13\n");
    let write = || fs::write(&path, "editor.font_size = 18\n").unwrap();
    if after_change(&path, write, font_size(18.)).is_none() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let path = settings_in(dir.path(), "editor.font_size = 13\n");
    let rename = || replace(&path, "editor.font_size = 18\n");
    if after_change(&path, rename, font_size(18.)).is_none() {
        return;
    }

    // From a loaded 18, invalid TOML keeps it.
    let dir = tempfile::tempdir().unwrap();
    let path = settings_in(dir.path(), "editor.font_size = 18\n");
    let invalid = || fs::write(&path, "editor.font_size = = 19\n").unwrap();
    let Some(store) = after_change(&path, invalid, has_error) else {
        return;
    };
    assert_eq!(store.settings().editor.font_size, 18.);
}

#[test]
fn symlink_targets_are_watched() {
    let dir = tempfile::tempdir().unwrap();
    let dotfiles = dir.path().join("dotfiles");
    fs::create_dir(&dotfiles).unwrap();
    let target = settings_in(&dotfiles, "editor.font_size = 13\n");
    let path = dir.path().join("settings.toml");
    symlink(&target, &path).unwrap();
    let edit = || fs::write(&target, "editor.font_size = 18\n").unwrap();
    after_change(&path, edit, font_size(18.));
}

#[test]
fn first_batch_names_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let (user_dir, project_dir) = (dir.path().join("user"), dir.path().join("project"));
    fs::create_dir(&user_dir).unwrap();
    fs::create_dir(&project_dir).unwrap();
    let user = settings_in(&user_dir, "editor.font_size = 13\n");
    let project = settings_in(&project_dir, "editor.tab_size = 2\n");
    let mut store = SettingsStore::load(Some(user.clone()), Some(project.clone()));
    // Between loading and watching, one file is saved and the other's
    // directory removed.
    fs::write(&user, "editor.font_size = 18\n").unwrap();
    fs::remove_dir_all(&project_dir).unwrap();
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(store.paths(), move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a first batch");
    assert_eq!(first, [user, project]);
    store.reload(&first).expect("both changes");
    assert_eq!(store.settings().editor.font_size, 18.);
    assert_eq!(store.settings().editor.tab_size, 4);
}
