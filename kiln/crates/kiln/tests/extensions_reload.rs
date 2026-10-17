// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::init` with an extensions directory: extensions load, and reload
//! as the config service hears of a change under the directory, headless.
//! The batches are sent as the tree watcher would send them.

use std::{cell::RefCell, fs, path::Path, rc::Rc};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{
        ApiVersion, ExtensionSet,
        extension::{ACKNOWLEDGED_FILE, MANIFEST_FILE},
    },
    platform::config::{self, batch_channel},
};
use serde_json::json;

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

fn manifest(id: &str, license: &str, lsp: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"{license}\"\n\n\
         [engine]\nnotes = \"1.0\"\n\n[contributes.lsp]\n{lsp}\n"
    )
}

fn install(root: &Path, dir: &str, text: &str) {
    let dir = root.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(MANIFEST_FILE), text).unwrap();
}

/// Each extension's ID and its `contributes.lsp`, as TOML.
type Tables = Vec<(String, String)>;

fn loaded_ids(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| kiln::extensions::loaded(cx))
        .into_iter()
        .map(|manifest| manifest.id)
        .collect()
}

#[gpui::test]
fn extensions_load_and_reload_through_init(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    install(
        &root,
        "one",
        &manifest("acme.one", "MIT", "command = \"one-ls\""),
    );

    // The lsp tables each call saw, by extension, as written.
    let seen: Rc<RefCell<Vec<Tables>>> = Rc::default();
    let notices: Rc<RefCell<Vec<String>>> = Rc::default();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .fail_fast(true)
        .extensions(&root, "notes", ApiVersion { major: 1, minor: 0 })
        .extension_kind(
            "lsp",
            json!({ "type": "object", "properties": { "command": { "type": "string" } } }),
        )
        .on_extensions({
            let seen = seen.clone();
            move |set: &ExtensionSet, _| {
                let tables = set
                    .extensions()
                    .iter()
                    .map(|ext| (ext.id.clone(), ext.app["lsp"].to_string()))
                    .collect();
                seen.borrow_mut().push(tables);
            }
        })
        .on_license_notice({
            let notices = notices.clone();
            move |notice, _| notices.borrow_mut().push(notice.message("Notes"))
        })
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.run_until_parked();
    assert_eq!(loaded_ids(cx), ["acme.one"]);
    assert_eq!(seen.borrow().len(), 1);

    install(
        &root,
        "two",
        &manifest(
            "acme.two",
            "GPL-3.0-only",
            "command = \"two-ls\"\nargs = [\"--stdio\"]",
        ),
    );
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(loaded_ids(cx), ["acme.one", "acme.two"]);
    assert_eq!(
        seen.borrow().last().unwrap(),
        &[
            ("acme.one".to_owned(), "{ command = \"one-ls\" }".to_owned()),
            (
                "acme.two".to_owned(),
                "{ args = [\"--stdio\"], command = \"two-ls\" }".to_owned()
            ),
        ]
    );
    assert_eq!(
        *notices.borrow(),
        ["acme.two (acme.two) is licensed GPL-3.0-only. Notes doesn't ship it; you installed it."]
    );

    // A batch naming only other files leaves the set be.
    sender.send(vec![state.path().join("settings.toml")]);
    cx.run_until_parked();
    assert_eq!(seen.borrow().len(), 2);

    // An error keeps the last good, and is in the config's diagnostics.
    install(&root, "one", "id = \"acme.one\"\nname = 3\n");
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(loaded_ids(cx), ["acme.one", "acme.two"]);
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error() && d.file == root.join("one").join(MANIFEST_FILE)),
        "{diagnostics:#?}"
    );
    assert_eq!(seen.borrow().len(), 3);
    assert_eq!(notices.borrow().len(), 1);

    // Nothing changed: nothing's called.
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(seen.borrow().len(), 3);
}

/// Sets up an app whose extensions are in `dir`, returning the licence
/// notices it gives, if it takes them.
fn notices_of(dir: &Path, take: bool, cx: &mut TestAppContext) -> Rc<RefCell<Vec<String>>> {
    let notices: Rc<RefCell<Vec<String>>> = Rc::default();
    let (_sender, batches) = batch_channel();
    let mut config = Config::new("com.example.Notes", "Notes")
        .extensions(dir, "notes", ApiVersion { major: 1, minor: 0 })
        .watch_with(batches);
    if take {
        let notices = notices.clone();
        config = config.on_license_notice(move |notice, _| {
            notices.borrow_mut().push(notice.id.clone());
        });
    }
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    notices
}

#[gpui::test]
fn licence_notices_wait_for_a_callback(first: &mut TestAppContext, second: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    install(
        &state.path().join("extensions"),
        "gpl",
        &manifest("acme.gpl", "GPL-3.0-only", "command = \"gpl-ls\""),
    );
    // Relative, as an app may give it: the state directory is still its
    // parent. The only test here that reads the working directory.
    std::env::set_current_dir(state.path()).unwrap();
    let relative = Path::new("extensions");

    // Nowhere to show it: it's only logged, and not acknowledged.
    assert_eq!(
        *notices_of(relative, false, first).borrow(),
        [] as [String; 0]
    );
    assert!(!state.path().join(ACKNOWLEDGED_FILE).exists());

    assert_eq!(*notices_of(relative, true, second).borrow(), ["acme.gpl"]);
    assert!(state.path().join(ACKNOWLEDGED_FILE).exists());
}
