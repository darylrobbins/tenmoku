// SPDX-License-Identifier: Apache-2.0

//! The watcher's own tests. Multi-step symlink changes feed a [`Tracker`]
//! events directly, with a hook standing in for changes made while a
//! directory is registered; the real-watcher test waits on the file system,
//! bounded by [`TIMEOUT`], asserting no duration. FSEvents delivery under
//! load isn't bounded (fseventsd is one daemon shared by every process), so
//! that wait runs beside a [`Control`], a raw stream over a directory of its
//! own: a timeout fails only if the control saw a sentinel written after the
//! change at least [`MARGIN`] before the deadline, and otherwise is noted and
//! passes. That holds only while nothing floods FSEvents, which drops events
//! silently then: the `fsevents` test group in `.config/nextest.toml` keeps
//! these tests apart from the 100k-file ones.

#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use notify::event::{CreateKind, Flag};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};
use tempfile::TempDir;

use crate::{ConfigWatcher, watch::Tracker};

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long [`project_edit_seen_while_registering`] waits for a batch before
/// rewriting the user's target again.
#[cfg(unix)]
const RETRY: Duration = Duration::from_secs(1);

/// How long before the deadline the control must have seen a sentinel for a
/// timeout to count as the watcher's failure.
const MARGIN: Duration = Duration::from_secs(10);

/// The control's readiness check: [`READY_RUN`] sentinels in a row, written
/// [`READY_GAP`] apart, all reported.
const READY_RUN: usize = 5;
const READY_GAP: Duration = Duration::from_millis(100);

/// Test code isn't shared between crates, so an app's own watch tests may
/// keep a copy of this.
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

/// `dir/settings.toml`, holding `text`.
fn settings_in(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("settings.toml");
    fs::write(&path, text).unwrap();
    path
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
#[cfg(unix)]
fn retarget(link: &Path, target: &Path) {
    let temp = link.with_extension("toml.tmp");
    symlink(target, &temp).unwrap();
    fs::rename(&temp, link).unwrap();
}

#[test]
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
/// the cause is unknown. As an empirical
/// mitigation the test rewrites the user's target every [`RETRY`] until a
/// batch names it, so it never depends on one write. The whole wait, batches
/// of any kind included, is bounded by [`TIMEOUT`], with a [`Control`],
/// ready before the project's edit, marked after it and after each rewrite.
#[cfg(unix)]
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
    let files = vec![user.clone(), project.clone()];

    // The project link moves before its directory is watched, so no event
    // can report it.
    let mut control = Control::new();
    let (tx, batches) = mpsc::channel();
    let hook_a = a.clone();
    let (hook_project, hook_c) = (project.clone(), to_c.clone());
    let hook_dir = dir.clone();
    let _watcher = ConfigWatcher::with_hook(
        files,
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
    let mut seen_project = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let what = if seen_project {
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
        // What a settings store would reload: the project's edit, read
        // through its link.
        let edited = batch.contains(&project)
            && fs::read_to_string(&project).unwrap() == "editor.font_size = 18\n";
        seen_project |= edited;
        seen_user |= batch.contains(&user);
        if seen_project && (seen_user || !await_user) {
            return;
        }
    }
}

/// Files set besides those given ([`Tracker::set_extra`]) are tracked in
/// their place: one in a directory not watched yet watches it and names
/// every file, one whose directory is watched names itself, and one no
/// longer set isn't reported.
#[test]
fn tracker_takes_up_extra_files() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let [a, b] = ["a", "b"].map(|name| dir.join(name));
    for sub in [&a, &b] {
        fs::create_dir(sub).unwrap();
    }
    let user = settings_in(&a, "");
    let project = settings_in(&b, "");
    let other = a.join("other.toml");
    let modify = EventKind::Modify(ModifyKind::Any);
    let none = |_: &Path| {};
    let set = |tracker: &mut Tracker, files: &[&PathBuf]| {
        let mut watched = Vec::new();
        let files = files.iter().map(|file| (*file).clone()).collect();
        let reported = tracker.set_extra(files, |dir| watched.push(dir.to_owned()));
        (reported, watched)
    };

    let mut tracker = Tracker::new(vec![user.clone()]);
    assert_eq!(start(&mut tracker, none), std::slice::from_ref(&a));
    let both = vec![user.clone(), project.clone()];
    assert_eq!(
        set(&mut tracker, &[&project]),
        (both.clone(), vec![b.clone()])
    );
    assert_eq!(tracker.files(), both);
    let batch = feed(&mut tracker, &[event(modify, &project)], none);
    assert_eq!(batch, (vec![project.clone()], vec![]));

    // Set again with another in a watched directory: only that one is new.
    let batch = set(&mut tracker, &[&project, &other]);
    assert_eq!(batch, (vec![other.clone()], vec![]));
    // A given file set too stays given, and isn't new.
    assert_eq!(set(&mut tracker, &[&user, &project]), (vec![], vec![]));

    // Unset, the project's edits aren't reported; set again, it's new.
    assert_eq!(set(&mut tracker, &[]), (vec![], vec![]));
    assert_eq!(tracker.files(), std::slice::from_ref(&user));
    let batch = feed(&mut tracker, &[event(modify, &project)], none);
    assert_eq!(batch, (vec![], vec![]));
    assert_eq!(tracker.reconcile(none), Vec::<PathBuf>::new());
    let batch = set(&mut tracker, &[&project]);
    assert_eq!(batch, (vec![project.clone()], vec![]));
    let batch = feed(&mut tracker, &[event(modify, &user)], none);
    assert_eq!(batch, (vec![user.clone()], vec![]));
}

/// A running watcher takes up a file set on it ([`ConfigWatcher::extra_files`])
/// at once, in a directory it didn't watch: the batch after setting it
/// names every file, and an edit to it is seen. Late fixture events may
/// name the user's file in batches between.
#[test]
fn watcher_takes_up_extra_files() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let [user_dir, project_dir] = ["user", "project"].map(|name| dir.join(name));
    for sub in [&user_dir, &project_dir] {
        fs::create_dir(sub).unwrap();
    }
    let user = settings_in(&user_dir, "");
    let project = settings_in(&project_dir, "");
    let mut control = Control::new();
    let (tx, batches) = mpsc::channel();
    let watcher = ConfigWatcher::new(vec![user.clone()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a first batch");
    assert_eq!(first, std::slice::from_ref(&user));
    let check = |batch: &[PathBuf]| {
        let mut files = [&user, &project].into_iter();
        assert!(
            !batch.is_empty() && batch.iter().all(|f| files.any(|expected| expected == f)),
            "a non-empty, in-order subset of [user, project], got {batch:?}",
        );
    };

    watcher.extra_files().set(vec![project.clone()]);
    loop {
        let batch = batches.recv_timeout(TIMEOUT).expect("a batch naming it");
        check(&batch);
        if batch.contains(&project) {
            assert_eq!(batch, [user.clone(), project.clone()]);
            break;
        }
    }

    if !control.ready("the project's edit") {
        return;
    }
    fs::write(&project, "editor.font_size = 18\n").unwrap();
    let marks = vec![control.mark()];
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(batch) = batches.recv_timeout(remaining) else {
            control.timed_out(&marks, deadline, "the project's edit");
            return;
        };
        check(&batch);
        if batch.contains(&project) {
            return;
        }
    }
}
