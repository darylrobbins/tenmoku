//! The real-watcher tests are the only ones that wait on the file system.
//! Each change gets a fresh watcher, started after its fixture is loaded, so
//! no batch from an earlier change can stand in for it. Waits are bounded by
//! [`TIMEOUT`], and no duration is asserted. Multi-step symlink changes feed
//! a [`Tracker`] events directly instead.

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

use crate::{
    ConfigWatcher, SettingsStore,
    watch::{Batch, Tracker},
};

/// An upper bound only: FSEvents may take a while on a busy machine.
const TIMEOUT: Duration = Duration::from_secs(10);

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

    let (mut tracker, first) = Tracker::new(files.clone());
    assert_eq!(
        first,
        Batch {
            files: files.clone(),
            new_dirs: vec![dir.clone(), a.clone()],
        }
    );
    // The target edited.
    let batch = tracker.batch(&[event(modify, &target)]);
    assert_eq!(batch.files, vec![link.clone()]);
    assert!(batch.new_dirs.is_empty());
    assert!(
        tracker
            .batch(&[event(modify, &dir.join("unrelated"))])
            .files
            .is_empty()
    );

    // The target replaced by a link to a missing file elsewhere: watching
    // that directory names every file, as registering it can drop events.
    let dangling = b.join("settings.toml");
    fs::remove_file(&target).unwrap();
    symlink(&dangling, &target).unwrap();
    let batch = tracker.batch(&[event(create, &target)]);
    assert_eq!(
        batch,
        Batch {
            files: files.clone(),
            new_dirs: vec![b.clone()],
        }
    );
    // The missing target created.
    fs::write(&dangling, "").unwrap();
    let batch = tracker.batch(&[event(create, &dangling)]);
    assert_eq!(batch.files, vec![link.clone()]);
    assert!(batch.new_dirs.is_empty());

    // The link itself replaced by a link elsewhere, then that target edited.
    let elsewhere = settings_in(&c, "");
    let temp_link = dir.join("settings.toml.tmp");
    symlink(&elsewhere, &temp_link).unwrap();
    fs::rename(&temp_link, &link).unwrap();
    let batch = tracker.batch(&[event(create, &link)]);
    assert_eq!(batch.new_dirs, vec![c.clone()]);
    let batch = tracker.batch(&[event(modify, &elsewhere)]);
    assert_eq!(batch.files, vec![link.clone()]);

    // A new directory re-resolves every file: another's link may have
    // changed while it was registered.
    let other_target = settings_in(&b, "");
    let d = dir.join("d");
    fs::create_dir(&d).unwrap();
    let d_target = settings_in(&d, "");
    symlink(&other_target, &other).unwrap();
    fs::remove_file(&elsewhere).unwrap();
    symlink(&d_target, &elsewhere).unwrap();
    let batch = tracker.batch(&[event(create, &elsewhere)]);
    assert_eq!(batch.files, files);
    assert_eq!(batch.new_dirs, vec![d.clone()]);
    let batch = tracker.batch(&[event(modify, &other_target)]);
    assert_eq!(batch.files, vec![other.clone()]);

    // Rescans and errors may have changed anything.
    let rescan = Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
    assert_eq!(tracker.batch(&[rescan]).files, files);
    let error = Err(notify::Error::generic("dropped"));
    assert_eq!(tracker.batch(&[error]).files, files);
}
