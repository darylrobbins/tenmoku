// SPDX-License-Identifier: LicenseRef-Proprietary

//! A file an app lists with [`kiln::Config::extension_files`] is noticed to
//! change by its contents, not only its size and modification time.

use std::{
    cell::Cell,
    fs::{self, File},
    rc::Rc,
};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE},
    platform::config::batch_channel,
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

/// An app with one extension, `acme.one`, in `root` whose `grammar.wasm` is
/// listed, counting the calls to `on_extensions`.
fn app(
    root: &std::path::Path,
) -> (
    Config,
    kiln::platform::config::BatchSender,
    Rc<Cell<usize>>,
    std::path::PathBuf,
) {
    let dir = root.join("one");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        "id = \"acme.one\"\nname = \"one\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\nnotes = \"1.0\"\n",
    )
    .unwrap();
    let module = dir.join("grammar.wasm");
    fs::write(&module, "aaaa").unwrap();
    let calls = Rc::new(Cell::new(0));
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .extensions(root, "notes", ApiVersion { major: 1, minor: 0 })
        .extension_files(|_| vec!["grammar.wasm".into()])
        .on_extensions({
            let calls = calls.clone();
            move |_, _| calls.set(calls.get() + 1)
        })
        .watch_with(batches);
    (config, sender, calls, module)
}

/// Rewrites `path` with `text` of the same length, its time restored.
fn rewrite(path: &std::path::Path, text: &str) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    fs::write(path, text).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

#[gpui::test]
fn a_rewrite_after_the_app_is_told_is_detected(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let (config, sender, calls, module) = app(&root);
    cx.update(|cx| kiln::init(cx, config));
    // The app hears of the extensions once their files are hashed.
    assert_eq!(calls.get(), 0);
    cx.run_until_parked();
    assert_eq!(calls.get(), 1);
    rewrite(&module, "bbbb");
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 2);

    // And again after a reload applied.
    rewrite(&module, "cccc");
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 3);
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 3);
}

#[gpui::test]
fn a_same_length_mtime_preserving_rewrite_is_detected(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let (config, sender, calls, module) = app(&root);
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    assert_eq!(calls.get(), 1);

    // Nothing changed: nothing's called.
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 1);

    // The same length, the same time.
    rewrite(&module, "bbbb");
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 2);

    // And that's the new baseline.
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(calls.get(), 2);
}
