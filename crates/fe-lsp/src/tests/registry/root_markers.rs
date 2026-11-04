//! Root markers: the built-in lists, the setting that replaces them and
//! `server_root`'s rule.

use std::fs;
use std::path::Path;

use fe_config::settings::{LspServerSettings, LspSettings};

use crate::registry::{self, Registry, server_root};

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

fn markers(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.into()).collect()
}

#[test]
fn server_root_follows_markers() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let file = ws.join("crates/a/src/lib.rs");
    let rust = markers(&["Cargo.lock", "Cargo.toml"]);
    touch(&file);

    // Under the window root: the window root.
    let window = dir.path().join("win");
    let inside = window.join("x/y.rs");
    assert_eq!(server_root(&inside, Some(&window), &rust), window);

    // A path that climbs out of the window root isn't under it.
    let escaping = window.join("../ws/crates/a/src/lib.rs");
    assert_eq!(
        server_root(&escaping, Some(&window), &rust),
        ws.join("crates/a/src")
    );

    // No marker above: the file's folder.
    assert_eq!(server_root(&file, None, &rust), ws.join("crates/a/src"));

    // Only the member's manifest.
    touch(&ws.join("crates/a/Cargo.toml"));
    assert_eq!(
        server_root(&file, Some(&window), &rust),
        ws.join("crates/a")
    );

    // The workspace's lock file beats the member's manifest.
    touch(&ws.join("Cargo.lock"));
    assert_eq!(server_root(&file, Some(&window), &rust), ws);
    assert_eq!(server_root(&file, None, &rust), ws);
}

#[test]
fn root_markers_come_from_builtins_and_settings() {
    let markers_of = |specs: &[crate::ServerSpec], name: &str| {
        specs
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .root_markers
            .clone()
    };
    let builtins = registry::builtin_specs();
    assert_eq!(
        markers_of(&builtins, "rust-analyzer"),
        markers(&["Cargo.lock", "Cargo.toml"])
    );
    assert_eq!(
        markers_of(&builtins, "pyright"),
        markers(&[
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "pyrightconfig.json",
            "requirements.txt"
        ])
    );
    let ts = markers(&["tsconfig.json", "jsconfig.json", "package.json"]);
    assert_eq!(markers_of(&builtins, "typescript-language-server"), ts);
    assert_eq!(markers_of(&builtins, "typescript-go"), ts);

    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        LspServerSettings {
            root_markers: Some(markers(&["x.toml"])),
            ..LspServerSettings::default()
        },
    );
    registry.set_settings(settings);
    assert_eq!(
        markers_of(&registry.specs(), "rust-analyzer"),
        markers(&["x.toml"])
    );
}

#[test]
fn unresolved_parent_components_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("project");
    touch(&proj.join("Cargo.toml"));
    let cargo = markers(&["Cargo.toml"]);
    assert_eq!(
        server_root(Path::new("../project/src/file.rs"), None, &[]),
        Path::new("../project/src")
    );
    assert_eq!(
        server_root(Path::new("../../a/file.rs"), None, &[]),
        Path::new("../../a")
    );
    assert_eq!(
        server_root(Path::new("a/../../b/file.rs"), None, &[]),
        Path::new("../b")
    );
    let up = proj.join("src/../../project/src/f.rs");
    assert_eq!(server_root(&up, None, &cargo), proj);
}
