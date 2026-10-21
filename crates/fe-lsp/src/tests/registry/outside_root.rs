//! Files outside the window root, or opened with none: each goes to a
//! server at its marker root (`server_root`), under the same trust rule.

use std::fs;
use std::path::{Path, PathBuf};

use fe_buffer::{Buffer, Origin};
use fe_config::settings::LspSettings;
use lsp_types::Url;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{barrier, open, routed};
use crate::fake::{FakeServer, Record};

/// A tree with window root `t/w` and a Cargo project at `t/other`, outside
/// it.
fn tree() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let window = dir.path().join("t/w");
    let other = dir.path().join("t/other");
    fs::create_dir_all(&window).unwrap();
    fs::create_dir_all(other.join("src")).unwrap();
    fs::write(other.join("Cargo.toml"), "").unwrap();
    (dir, window, other)
}

fn path(root: &Path, name: &str) -> String {
    root.join(name).to_str().unwrap().to_owned()
}

fn folder_uri(root: &Path) -> Value {
    json!(Url::from_file_path(root).unwrap())
}

/// The `initialize` params launch `launch` (from 1) got.
fn initialize(fake: &FakeServer, launch: usize) -> Value {
    let records = fake.wait_for_at(launch, "initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    records
        .into_iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap()
}

/// Waits until launch `launch` has the document at `path` open.
fn wait_opened(fake: &FakeServer, path: &str, launch: usize) {
    let uri = Url::from_file_path(path).unwrap();
    fake.wait_for_document(&uri, "didOpen", |r| {
        r.is_some_and(|r| r.open && r.launch == launch)
    });
}

#[test]
fn files_outside_the_root_get_their_marker_roots_server() {
    let (_dir, window, other) = tree();
    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());
    let trusted = [window.clone(), other.clone()];
    manager.set_trust(move |root| trusted.iter().any(|t| t == root));

    let mut inside = Buffer::new("fn a() {}\n");
    let inside_path = path(&window, "src/a.rs");
    let at_window = open(
        &mut manager,
        Some(&window),
        &mut inside,
        &inside_path,
        "rust",
    )
    .unwrap();
    wait_opened(&fake, &inside_path, 1);

    // A file under the Cargo project outside the window root starts a
    // second server there.
    let mut main = Buffer::new("fn main() {}\n");
    let main_path = path(&other, "src/main.rs");
    let at_other = open(&mut manager, Some(&window), &mut main, &main_path, "rust").unwrap();
    assert_ne!(at_other, at_window);
    assert_eq!(manager.server_of(main.id()), Some(at_other));
    wait_opened(&fake, &main_path, 2);
    let launched = fake.launched();
    assert_eq!(launched.len(), 2);
    assert_eq!(launched[0].1, window);
    assert_eq!(launched[1].1, other);
    let params = initialize(&fake, 2);
    assert_eq!(params["rootUri"], folder_uri(&other));
    assert_eq!(params["workspaceFolders"][0]["uri"], folder_uri(&other));
    assert_eq!(initialize(&fake, 1)["rootUri"], folder_uri(&window));

    // Another file there joins it.
    let mut lib = Buffer::new("pub fn lib() {}\n");
    let lib_path = path(&other, "src/lib.rs");
    assert_eq!(
        open(&mut manager, Some(&window), &mut lib, &lib_path, "rust"),
        Some(at_other)
    );
    wait_opened(&fake, &lib_path, 2);

    // One under the window root still goes to the window root's server.
    let mut more = Buffer::new("fn b() {}\n");
    let more_path = path(&window, "src/b.rs");
    assert_eq!(
        open(&mut manager, Some(&window), &mut more, &more_path, "rust"),
        Some(at_window)
    );
    wait_opened(&fake, &more_path, 1);
    barrier(&manager, at_window);
    barrier(&manager, at_other);
    assert_eq!(fake.launches(), 2);
}

#[test]
fn untrusted_marker_roots_wait_for_trust() {
    let (_dir, window, other) = tree();
    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());
    let only = window.clone();
    manager.set_trust(move |root| root == only);

    let mut main = Buffer::new("fn main() {}\n");
    let main_path = path(&other, "src/main.rs");
    assert_eq!(
        open(&mut manager, Some(&window), &mut main, &main_path, "rust"),
        None
    );
    assert_eq!(manager.server_of(main.id()), None);
    assert_eq!(fake.launches(), 0);

    // Kept meanwhile: trusting its root starts a server there, which opens
    // it at its current text.
    main.edit([(0..0, "// x\n")], Origin::User).unwrap();
    manager.changed(main.id());
    let trusted = [window.clone(), other.clone()];
    manager.set_trust(move |root| trusted.iter().any(|t| t == root));
    let server = manager.server_of(main.id()).unwrap();
    let uri = Url::from_file_path(&main_path).unwrap();
    let replica = fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    assert_eq!(replica.unwrap().text, "// x\nfn main() {}\n");
    barrier(&manager, server);
    let launched = fake.launched();
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].1, other);
}

#[test]
fn no_window_root_still_routes() {
    let (_dir, _window, other) = tree();
    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());

    let mut main = Buffer::new("fn main() {}\n");
    let main_path = path(&other, "src/main.rs");
    let server = open(&mut manager, None, &mut main, &main_path, "rust").unwrap();
    assert_eq!(manager.server_of(main.id()), Some(server));
    wait_opened(&fake, &main_path, 1);
    assert_eq!(initialize(&fake, 1)["rootUri"], folder_uri(&other));
    let launched = fake.launched();
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].1, other);
}
