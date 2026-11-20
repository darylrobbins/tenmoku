// SPDX-License-Identifier: Apache-2.0

//! The paths an app lists with [`kiln::Config::extension_files`] are
//! recorded as entries: by path, type and link target, so a retargeted link
//! or an added or removed directory is a change, and the listing and the
//! hashing never run on the main thread nor read a big file whole.

use std::{
    cell::Cell,
    fs::{self, File},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE},
    platform::config::{BatchSender, batch_channel},
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

/// An app with one extension, `acme.one`, in `root`, which lists `files`,
/// counting the calls to `on_extensions`.
fn app(root: &Path, files: Vec<PathBuf>) -> (Config, BatchSender, Rc<Cell<usize>>, PathBuf) {
    app_with(root, move |_| files.clone())
}

fn app_with(
    root: &Path,
    files: impl Fn(&kiln::core::Manifest) -> Vec<PathBuf> + Send + Sync + 'static,
) -> (Config, BatchSender, Rc<Cell<usize>>, PathBuf) {
    let dir = root.join("one");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        "id = \"acme.one\"\nname = \"one\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\nnotes = \"1.0\"\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .extensions(root, "notes", ApiVersion { major: 1, minor: 0 })
        .extension_files(files)
        .on_extensions({
            let calls = calls.clone();
            move |_, _| calls.set(calls.get() + 1)
        })
        .watch_with(batches);
    (config, sender, calls, dir)
}

fn start(cx: &mut TestAppContext, config: Config) {
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
}

/// Sends a batch for `root` and settles.
fn batch(cx: &mut TestAppContext, sender: &BatchSender, root: &Path) {
    sender.send(vec![root.to_owned()]);
    cx.run_until_parked();
}

/// Starts `app` with `files` listed, makes `change`, and shows it calls
/// `on_extensions` again, once, and that a batch after it calls nothing.
fn changes_are_seen(
    cx: &mut TestAppContext,
    files: &[&str],
    setup: impl FnOnce(&Path),
    change: impl FnOnce(&Path),
) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let files = files.iter().map(PathBuf::from).collect();
    let (config, sender, calls, dir) = app(&root, files);
    setup(&dir);
    start(cx, config);
    assert_eq!(calls.get(), 1);
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 1, "nothing changed");
    change(&dir);
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 2, "the change is seen");
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 2, "and is the new baseline");
}

#[cfg(unix)]
fn relink(link: &Path, target: &Path) {
    let _ = fs::remove_file(link);
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(unix)]
#[gpui::test]
fn a_retargeted_directory_symlink_is_a_change(cx: &mut TestAppContext) {
    changes_are_seen(
        cx,
        &["link"],
        |dir| {
            fs::create_dir_all(dir.join("a")).unwrap();
            fs::create_dir_all(dir.join("b")).unwrap();
            relink(&dir.join("link"), Path::new("a"));
        },
        |dir| relink(&dir.join("link"), Path::new("b")),
    );
}

#[cfg(unix)]
#[gpui::test]
fn a_retargeted_escaping_link_is_a_change(cx: &mut TestAppContext) {
    let outside = tempfile::tempdir().unwrap();
    let (x, y) = (outside.path().join("x"), outside.path().join("y"));
    fs::write(&x, "x").unwrap();
    fs::write(&y, "y").unwrap();
    changes_are_seen(
        cx,
        &["link"],
        |dir| relink(&dir.join("link"), &x),
        |dir| relink(&dir.join("link"), &y),
    );
}

#[cfg(unix)]
#[gpui::test]
fn a_retargeted_dangling_link_is_a_change(cx: &mut TestAppContext) {
    changes_are_seen(
        cx,
        &["link"],
        |dir| relink(&dir.join("link"), Path::new("nowhere-1")),
        |dir| relink(&dir.join("link"), Path::new("nowhere-2")),
    );
}

#[gpui::test]
fn an_added_listed_directory_is_a_change(cx: &mut TestAppContext) {
    changes_are_seen(
        cx,
        &["empty"],
        |_| {},
        |dir| fs::create_dir(dir.join("empty")).unwrap(),
    );
}

#[gpui::test]
fn a_removed_listed_directory_is_a_change(cx: &mut TestAppContext) {
    changes_are_seen(
        cx,
        &["empty"],
        |dir| fs::create_dir(dir.join("empty")).unwrap(),
        |dir| fs::remove_dir(dir.join("empty")).unwrap(),
    );
}

#[cfg(unix)]
#[gpui::test]
fn a_directory_that_becomes_unlistable_is_a_change(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let (config, sender, calls, dir) = app(&root, vec!["locked".into()]);
    let locked = dir.join("locked");
    fs::create_dir(&locked).unwrap();
    start(cx, config);
    assert_eq!(calls.get(), 1);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let listable = fs::read_dir(&locked).is_ok();
    batch(cx, &sender, &root);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if listable {
        // Running as root: the mode changes nothing.
        return;
    }
    assert_eq!(calls.get(), 2);
}

#[gpui::test]
fn extension_files_are_listed_off_the_main_thread(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let on_main = Arc::new(Mutex::new(Vec::new()));
    let executor = cx.executor().clone();
    let (config, sender, calls, dir) = app_with(&root, {
        let on_main = on_main.clone();
        move |_| {
            on_main.lock().unwrap().push(executor.is_main_thread());
            vec!["grammar.wasm".into()]
        }
    });
    fs::write(dir.join("grammar.wasm"), "aaaa").unwrap();
    // At install.
    start(cx, config);
    assert_eq!(calls.get(), 1);
    let at_install = on_main.lock().unwrap().len();
    assert!(at_install >= 1);
    // On reload, and on apply after a change.
    batch(cx, &sender, &root);
    fs::write(dir.join("grammar.wasm"), "bbbb").unwrap();
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 2);
    let seen = on_main.lock().unwrap().clone();
    assert!(seen.len() > at_install, "{seen:?}");
    assert_eq!(seen.iter().filter(|main| **main).count(), 0, "{seen:?}");
}

#[gpui::test]
fn a_walk_returning_after_a_newer_reload_began_is_dropped(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let walks = Arc::new(AtomicU64::new(0));
    // The install's walk lists `old`; every later one lists `new`.
    let (config, sender, calls, _dir) = app_with(&root, {
        let walks = walks.clone();
        move |_| {
            let n = walks.fetch_add(1, Ordering::SeqCst);
            vec![if n == 0 { "old" } else { "new" }.into()]
        }
    });
    cx.update(|cx| kiln::init(cx, config));
    // The install's walk is in flight, unsettled, when a reload starts a
    // newer one.
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(walks.load(Ordering::SeqCst), 2);
    // Only the newer listing is applied, once.
    assert_eq!(calls.get(), 1);
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 1);
}

#[gpui::test]
fn a_big_file_link_and_named_path_are_listed_and_seen_by_size(cx: &mut TestAppContext) {
    const OVER: u64 = 320 * 1024 * 1024;
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let files = vec![
        PathBuf::from("big.bin"),
        #[cfg(unix)]
        PathBuf::from("link.bin"),
        PathBuf::from("args/named.bin"),
    ];
    let (config, sender, calls, dir) = app(&root, files);
    for path in ["big.bin", "args/named.bin"] {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        File::create(path).unwrap().set_len(OVER).unwrap();
    }
    #[cfg(unix)]
    relink(&dir.join("link.bin"), Path::new("big.bin"));
    // How little of them is read is pinned by the `hash_tests` unit tests.
    start(cx, config);
    assert_eq!(calls.get(), 1);
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 1);
    // Still seen to change, by size.
    File::options()
        .write(true)
        .open(dir.join("big.bin"))
        .unwrap()
        .set_len(OVER + 1)
        .unwrap();
    batch(cx, &sender, &root);
    assert_eq!(calls.get(), 2);
}
