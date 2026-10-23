// SPDX-License-Identifier: LicenseRef-Proprietary

//! A workspace's extensions: [`ExtensionApi::only_kinds`] keeps the kinds
//! a workspace may contribute and drops the rest with a warning, and a set
//! joined after the user's loses an ID the user's has.

use std::{fs, path::Path};

use kiln_core::{
    ApiVersion, AppKind, ExtensionApi, ExtensionSet, Severity,
    extension::{ExtensionSource, MANIFEST_FILE},
    load_manifest,
};
use serde_json::json;

/// A notes app with an `lsp` kind and a `grammars` kind; a workspace's
/// extensions may contribute only `lsp`, if `only`.
fn api(only: bool) -> ExtensionApi {
    let kind = |name| AppKind {
        name,
        schema: json!({ "type": "object" }),
    };
    ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 0 },
        reserved: Vec::new(),
        kinds: vec![kind("lsp"), kind("grammars")],
        allowed_licenses: None,
        only_kinds: only.then(|| vec!["lsp".into()]),
    }
}

fn write_manifest(dir: &Path, id: &str, contributes: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{id}\"\nname = \"Zig\"\nversion = \"1.0.0\"\nlicense = \"GPL-3.0-only\"\n\n\
             [engine]\nnotes = \"1.0\"\n\n{contributes}"
        ),
    )
    .unwrap();
}

/// Contributes a theme, a keymap, a menu, a grammar and a language server;
/// the theme's path leads outside the directory, which would be an error.
const EVERYTHING: &str = "[contributes]\nthemes = [\"../outside.toml\"]\n\
    keymaps = [\"keys.toml\"]\nmenus = [\"menus.toml\"]\n\n\
    [contributes.grammars]\nzig = \"zig.wasm\"\n\n\
    [contributes.lsp.zls]\ncommand = \"zls\"\n";

#[test]
fn a_workspace_manifest_keeps_only_language_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let zig = dir.path().join("zig");
    write_manifest(&zig, "acme.zig", EVERYTHING);
    fs::write(zig.join("keys.toml"), "").unwrap();
    fs::write(zig.join("menus.toml"), "").unwrap();

    let (manifest, diagnostics) = load_manifest(&zig, &api(true));
    let manifest = manifest.expect("it loads");
    assert!(manifest.themes.is_empty());
    assert!(manifest.keymaps.is_empty());
    assert!(manifest.menus.is_empty());
    assert_eq!(
        manifest.app.keys().collect::<Vec<_>>(),
        ["lsp"],
        "only the allowed app kind is kept"
    );
    assert_eq!(manifest.app["lsp"]["zls"]["command"].as_str(), Some("zls"));
    let found: Vec<(Severity, Option<&str>, &str)> = (diagnostics.iter())
        .map(|d| (d.severity, d.key.as_deref(), d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                Severity::Warning,
                Some("contributes.themes"),
                "workspace extensions can't contribute themes, so `contributes.themes` is ignored"
            ),
            (
                Severity::Warning,
                Some("contributes.keymaps"),
                "workspace extensions can't contribute keymaps, so `contributes.keymaps` is ignored"
            ),
            (
                Severity::Warning,
                Some("contributes.menus"),
                "workspace extensions can't contribute menus, so `contributes.menus` is ignored"
            ),
            (
                Severity::Warning,
                Some("contributes.grammars"),
                "workspace extensions can't contribute grammars, so `contributes.grammars` is ignored"
            ),
        ]
    );
    assert!(diagnostics.iter().all(|d| d.line > 1), "each at its key");

    // Without `only_kinds`, the same manifest has the theme's error.
    let (manifest, diagnostics) = load_manifest(&zig, &api(false));
    assert_eq!(manifest, None);
    assert!(diagnostics.iter().any(|d| d.is_error()), "{diagnostics:?}");
}

#[test]
fn a_joined_set_keeps_the_users_copy_of_an_id() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let workspace = dir.path().join("workspace");
    write_manifest(&user.join("zig"), "acme.zig", "");
    write_manifest(&workspace.join("zig"), "acme.zig", "");
    write_manifest(&workspace.join("odin"), "acme.odin", "");

    let user = ExtensionSet::discover(&user, &api(false));
    let workspace = ExtensionSet::discover_from(&workspace, &api(true), ExtensionSource::Workspace);
    assert_eq!(user.source(), ExtensionSource::User);
    assert_eq!(workspace.source(), ExtensionSource::Workspace);
    let joined = user.joined(&workspace);
    let loaded: Vec<(ExtensionSource, &str)> = (joined.sourced())
        .map(|(source, manifest)| (source, manifest.id.as_str()))
        .collect();
    assert_eq!(
        loaded,
        [
            (ExtensionSource::User, "acme.zig"),
            (ExtensionSource::Workspace, "acme.odin"),
        ]
    );
    let [warning] = joined.diagnostics() else {
        panic!("one warning: {:?}", joined.diagnostics());
    };
    let shadowed = workspace.extensions().iter().find(|m| m.id == "acme.zig");
    let shadowed = shadowed.unwrap().dir.clone();
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.file, shadowed.join(MANIFEST_FILE));
    assert_eq!(warning.key.as_deref(), Some("id"));
    assert_eq!(
        warning.message,
        format!(
            "the workspace's acme.zig in {} isn't loaded: the user's copy in {} loads instead",
            shadowed.display(),
            user.extensions()[0].dir.display()
        )
    );
    // Only the user's own extensions have licence notices.
    let state = tempfile::tempdir().unwrap();
    let notices = joined.pending_license_notices(state.path());
    let ids: Vec<&str> = notices.iter().map(|notice| notice.id.as_str()).collect();
    assert_eq!(ids, ["acme.zig"]);
}
