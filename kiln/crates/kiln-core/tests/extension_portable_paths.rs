// SPDX-License-Identifier: Apache-2.0

//! A contributed path is written with `/` on every OS.

use std::fs;

use kiln_core::{ApiVersion, ExtensionApi, extension::MANIFEST_FILE, load_manifest};

fn load(path: &str) -> (Vec<String>, bool) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("a")).unwrap();
    fs::write(dir.path().join("a/b.wasm"), "").unwrap();
    let manifest = format!(
        "id = \"acme.x\"\nname = \"X\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n[engine]\nnotes = \"1.0\"\n\n[contributes]\nmenus = [{path:?}]\n"
    );
    fs::write(dir.path().join(MANIFEST_FILE), manifest).unwrap();
    let api = ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 0 },
        reserved: vec![],
        kinds: vec![],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (loaded, diagnostics) = load_manifest(dir.path(), &api);
    (
        diagnostics.iter().map(|d| d.message.clone()).collect(),
        loaded.is_some(),
    )
}

#[test]
fn extension_portable_paths_refuse_backslash() {
    let (messages, loaded) = load("a\\b.wasm");
    assert!(!loaded);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use / in extension paths")),
        "{messages:?}"
    );
    let (messages, loaded) = load("a/b.wasm");
    assert!(loaded, "{messages:?}");
}
