//! The real-watcher tests are the only ones that wait on the file system.
//! Each change gets a fresh watcher, started after its fixture is loaded, so
//! no batch from an earlier change can stand in for it. Waits are bounded by
//! [`TIMEOUT`], and no duration is asserted. Multi-step symlink changes feed
//! a [`Tracker`] events directly instead, with a hook standing in for
//! changes made while a directory is registered.

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use notify::{
    Event, EventKind,
    event::{CreateKind, Flag, ModifyKind},
};

use crate::{ConfigWatcher, SettingsStore, watch::Tracker};

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Loads `path`, starts a fresh watcher over it, takes its first batch, then
/// makes `change` and reloads each batch naming `path` until `done` holds.
/// One save can arrive in several batches, the first maybe before the write
/// finishes.
fn after_change(
    path: &Path,
    change: impl FnOnce(),
    done: impl Fn(&SettingsStore) -> bool,
) -> SettingsStore {
    let mut store = SettingsStore::load(Some(path.to_owned()), None);
    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(vec![path.to_owned()], move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let batch = || batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(batch(), [path], "the first batch, before any event");
    change();
    loop {
        let batch = batch();
        assert_eq!(batch, [path]);
        store.reload(&batch);
        if done(&store) {
            return store;
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
    after_change(&path, write, font_size(18.));

    let dir = tempfile::tempdir().unwrap();
    let path = settings_in(dir.path(), "editor.font_size = 13\n");
    let rename = || replace(&path, "editor.font_size = 18\n");
    after_change(&path, rename, font_size(18.));

    // From a loaded 18, invalid TOML keeps it.
    let dir = tempfile::tempdir().unwrap();
    let path = settings_in(dir.path(), "editor.font_size = 18\n");
    let invalid = || fs::write(&path, "editor.font_size = = 19\n").unwrap();
    let store = after_change(&path, invalid, has_error);
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
/// waits for a later batch naming the user file, which the caller's `extra`
/// guarantees, so the tolerant loop certainly takes one.
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
    let batch = || batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(batch(), [user.clone(), project.clone()]);
    fs::write(&to_c, "editor.font_size = 18\n").unwrap();
    let mut seen_user = false;
    loop {
        let batch = batch();
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
