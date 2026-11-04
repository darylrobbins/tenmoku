//! gopls, the built-in Go server.

use std::fs;
use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use serde_json::json;

use super::{open, root, routed, server, strings, uri};
use crate::document::language_id;
use crate::registry::{self, Registry, server_for, server_root};
use crate::tests::ROOT;

#[test]
fn go_file_gets_gopls_by_default() {
    let specs = Registry::default().specs();
    let gopls = server_for(&specs, "go").unwrap();
    assert_eq!(gopls.name, "gopls");
    assert_eq!(gopls.command.to_str(), Some("gopls"));
    assert!(gopls.args.is_empty());
    assert_eq!(
        server_for(&specs, "rust").map(|s| s.name.as_str()),
        Some("rust-analyzer")
    );
}

#[test]
fn go_work_beats_go_mod() {
    let dir = tempfile::tempdir().unwrap();
    let markers = registry::builtin_specs()
        .into_iter()
        .find(|s| s.name == "gopls")
        .unwrap()
        .root_markers;
    assert_eq!(markers, strings(&["go.work", "go.mod"]));
    let work = dir.path().join("work");
    let file = work.join("mod/pkg/a.go");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "").unwrap();

    assert_eq!(server_root(&file, None, &markers), work.join("mod/pkg"));
    fs::write(work.join("mod/go.mod"), "").unwrap();
    assert_eq!(server_root(&file, None, &markers), work.join("mod"));
    fs::write(work.join("go.work"), "").unwrap();
    assert_eq!(server_root(&file, None, &markers), work);
}

#[test]
fn settings_can_disable_or_override_gopls() {
    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("gopls".into(), server(json!({ "enabled": false })));
    registry.set_settings(settings);
    assert!(server_for(&registry.specs(), "go").is_none());

    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("gopls".into(), server(json!({ "command": "/opt/gopls" })));
    registry.set_settings(settings);
    let specs = registry.specs();
    let gopls = server_for(&specs, "go").unwrap();
    assert_eq!(gopls.command.to_str(), Some("/opt/gopls"));
    assert_eq!(gopls.languages, strings(&["go"]));
    assert_eq!(gopls.root_markers, strings(&["go.work", "go.mod"]));
}

#[test]
fn go_language_id_is_go() {
    let path = PathBuf::from(ROOT).join("main.go");
    assert_eq!(language_id("go", &path), "go");

    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());
    let mut buffer = Buffer::new("package main\n");
    let name = format!("{ROOT}/main.go");
    open(&mut manager, root(), &mut buffer, &name, "go").expect("a server takes it");
    fake.wait_for_document(&uri(&name), "didOpen", |r| r.is_some());
    assert_eq!(fake.document(&uri(&name)).unwrap().language_id, "go");
}
