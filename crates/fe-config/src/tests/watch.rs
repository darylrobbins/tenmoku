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
//! before. Multi-step symlink changes feed a [`Tracker`] events directly
//! instead, with a hook standing in for changes made while a directory is
//! registered.

use std::{
    collections::HashMap,
    fs,
    os::unix::fs::symlink,
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

use crate::{ConfigWatcher, SettingsStore, watch::Tracker};

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long [`project_edit_seen_while_registering`] waits for a batch before
/// rewriting the user's target again.
const RETRY: Duration = Duration::from_secs(1);

/// How long before the deadline the control must have seen a sentinel for a
/// timeout to count as the watcher's failure.
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

fn event(kind: EventKind, path: &Path) -> notify::Result<Event> {
    Ok(Event::new(kind).add_path(path.to_owned()))
}

/// The directories `tracker` starts watching, calling `during` with each.
fn start(tracker: &mut Tracker, mut during: impl FnMut(&Path)) -> Vec<PathBuf> {
    let mut watched = Vec::new();
    let Ok(_) = tracker.register(|dir| {
        watched.push(dir.to_owned());
        during(dir);
        Ok::<_, std::convert::Infallible>(())
    });
    watched
}

/// The files `tracker` reports for `events`, and the directories it starts
/// watching, calling `during` with each.
fn feed(
    tracker: &mut Tracker,
    events: &[notify::Result<Event>],
    mut during: impl FnMut(&Path),
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut watched = Vec::new();
    let files = tracker.batch(events, |dir| {
        watched.push(dir.to_owned());
        during(dir);
    });
    (files, watched)
}

/// Points the symlink `link` at `target`, replacing it atomically.
fn retarget(link: &Path, target: &Path) {
    let temp = link.with_extension("toml.tmp");
    symlink(target, &temp).unwrap();
    fs::rename(&temp, link).unwrap();
}

#[test]
fn tracker_follows_symlink_changes() {
    let temp = tempfile::tempdir().unwrap();
    // Canonical, as events name paths.
    let dir = temp.path().canonicalize().unwrap();
    let (a, b, c) = (dir.join("a"), dir.join("b"), dir.join("c"));
    for sub in [&a, &b, &c] {
        fs::create_dir(sub).unwrap();
    }
    let target = settings_in(&a, "");
    let link = dir.join("settings.toml");
    symlink(&target, &link).unwrap();
    let other = dir.join("other.toml");
    let files = vec![link.clone(), other.clone()];
    let modify = EventKind::Modify(ModifyKind::Any);
    let create = EventKind::Create(CreateKind::File);
    let none = |_: &Path| {};

    let mut tracker = Tracker::new(files.clone());
    assert_eq!(tracker.files(), files);
    assert_eq!(start(&mut tracker, none), [dir.clone(), a.clone()]);
    // The target edited.
    let batch = feed(&mut tracker, &[event(modify, &target)], none);
    assert_eq!(batch, (vec![link.clone()], vec![]));
    let unrelated = [event(modify, &dir.join("unrelated"))];
    assert_eq!(feed(&mut tracker, &unrelated, none), (vec![], vec![]));

    // The target replaced by a link to a missing file elsewhere: watching
    // that directory names every file, as registering it can drop events.
    let dangling = b.join("settings.toml");
    fs::remove_file(&target).unwrap();
    symlink(&dangling, &target).unwrap();
    let batch = feed(&mut tracker, &[event(create, &target)], none);
    assert_eq!(batch, (files.clone(), vec![b.clone()]));
    // The missing target created.
    fs::write(&dangling, "").unwrap();
    let batch = feed(&mut tracker, &[event(create, &dangling)], none);
    assert_eq!(batch, (vec![link.clone()], vec![]));

    // The link itself replaced by a link elsewhere, then that target edited.
    let elsewhere = settings_in(&c, "");
    retarget(&link, &elsewhere);
    let batch = feed(&mut tracker, &[event(create, &link)], none);
    assert_eq!(batch, (files.clone(), vec![c.clone()]));
    let batch = feed(&mut tracker, &[event(modify, &elsewhere)], none);
    assert_eq!(batch, (vec![link.clone()], vec![]));

    // Rescans and errors may have changed anything.
    let rescan = Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
    assert_eq!(feed(&mut tracker, &[rescan], none).0, files);
    let error = Err(notify::Error::generic("dropped"));
    assert_eq!(feed(&mut tracker, &[error], none).0, files);
}

/// Another file's link changing while a directory is registered, its event
/// lost, is followed all the same: every link is resolved again after
/// registering, at startup and later.
#[test]
fn tracker_resolves_links_after_registering() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let [a, b, c, d, e] = ["a", "b", "c", "d", "e"].map(|name| dir.join(name));
    for sub in [&a, &b, &c, &d, &e] {
        fs::create_dir(sub).unwrap();
    }
    let (user, project) = (dir.join("user.toml"), dir.join("project.toml"));
    symlink(settings_in(&a, ""), &user).unwrap();
    symlink(settings_in(&b, ""), &project).unwrap();
    let files = vec![user.clone(), project.clone()];
    let modify = EventKind::Modify(ModifyKind::Any);
    let create = EventKind::Create(CreateKind::File);

    // At startup, registering the user target's directory, the project
    // link moves to c.
    let mut tracker = Tracker::new(files.clone());
    let to_c = settings_in(&c, "");
    let watched = start(&mut tracker, |watched| {
        if watched == a {
            retarget(&project, &to_c);
        }
    });
    assert_eq!(watched, [dir.clone(), a.clone(), b.clone(), c.clone()]);
    let batch = feed(&mut tracker, &[event(modify, &to_c)], |_| {});
    assert_eq!(batch, (vec![project.clone()], vec![]));

    // Later, the user link moves to d, and registering d, the project link
    // moves to e.
    let to_e = settings_in(&e, "");
    retarget(&user, &settings_in(&d, ""));
    let batch = feed(&mut tracker, &[event(create, &user)], |watched| {
        if watched == d {
            retarget(&project, &to_e);
        }
    });
    assert_eq!(batch, (files.clone(), vec![d.clone(), e.clone()]));
    let batch = feed(&mut tracker, &[event(modify, &to_e)], |_| {});
    assert_eq!(batch, (vec![project.clone()], vec![]));
}

/// The same at startup with a real watcher: an edit to the project link's
/// new target is seen.
#[test]
fn watcher_follows_links_changed_while_registering() {
    project_edit_seen_while_registering(false, |_, _| {});
}

/// Runs the test above with `extra` called, after the project link's move,
/// for each directory registered, with the fixture's root.
///
/// Fixture and hook events can arrive late, after the first batch, so later
/// batches may name the unchanged user file too. Each is reloaded, and the
/// loop ends once the project's edit is loaded. With `await_user`, it also
/// waits for a later batch naming the user file. The caller's `extra` may
/// cause one, but under heavy load a single write has gone unreported, and
/// the cause is unknown (see the T211 spec's notes). As an empirical
/// mitigation the test rewrites the user's target every [`RETRY`] until a
/// batch names it, so it never depends on one write. The whole wait, batches
/// of any kind included, is bounded by [`TIMEOUT`], with a [`Control`],
/// ready before the project's edit, marked after it and after each rewrite.
pub(super) fn project_edit_seen_while_registering(
    await_user: bool,
    extra: impl Fn(&Path, &Path) + Send + 'static,
) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let [user_dir, a, project_dir, b, c] =
        ["user", "a", "project", "b", "c"].map(|name| dir.join(name));
    for sub in [&user_dir, &a, &project_dir, &b, &c] {
        fs::create_dir(sub).unwrap();
    }
    let user = user_dir.join("settings.toml");
    let project = project_dir.join("settings.toml");
    symlink(settings_in(&a, ""), &user).unwrap();
    symlink(settings_in(&b, "editor.font_size = 13\n"), &project).unwrap();
    let to_c = settings_in(&c, "editor.font_size = 13\n");
    let mut store = SettingsStore::load(Some(user.clone()), Some(project.clone()));

    // The project link moves before its directory is watched, so no event
    // can report it.
    let mut control = Control::new();
    let (tx, batches) = mpsc::channel();
    let hook_a = a.clone();
    let (hook_project, hook_c) = (project.clone(), to_c.clone());
    let hook_dir = dir.clone();
    let _watcher = ConfigWatcher::with_hook(
        store.paths(),
        move |batch| {
            tx.send(batch).ok();
        },
        move |watched| {
            if watched == hook_a {
                retarget(&hook_project, &hook_c);
            }
            extra(watched, &hook_dir);
        },
    )
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a first batch");
    assert_eq!(first, [user.clone(), project.clone()]);
    if !control.ready("the project's edit") {
        return;
    }
    fs::write(&to_c, "editor.font_size = 18\n").unwrap();
    let mut marks = vec![control.mark()];
    let deadline = Instant::now() + TIMEOUT;
    let mut seen_user = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let what = if store.settings().editor.font_size == 18. {
                "the user's rewrite"
            } else {
                "the project's edit"
            };
            control.timed_out(&marks, deadline, what);
            return;
        }
        let retrying = await_user && !seen_user;
        let wait = if retrying {
            RETRY.min(remaining)
        } else {
            remaining
        };
        let batch = match batches.recv_timeout(wait) {
            Ok(batch) => batch,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if retrying {
                    settings_in(&a, "");
                    marks.push(control.mark());
                }
                continue;
            }
            Err(error) => panic!("a batch in time: {error}"),
        };
        let mut files = [&user, &project].into_iter();
        assert!(
            !batch.is_empty() && batch.iter().all(|f| files.any(|expected| expected == f)),
            "a non-empty, in-order subset of [user, project], got {batch:?}",
        );
        store.reload(&batch);
        seen_user |= batch.contains(&user);
        if store.settings().editor.font_size == 18. && (seen_user || !await_user) {
            return;
        }
    }
}
