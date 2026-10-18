// SPDX-License-Identifier: LicenseRef-Proprietary

//! Loading `extension.toml` manifests, written into temporary directories.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use kiln_core::{
    ApiVersion, AppId, AppKind, AppPaths, Diagnostic, ExtensionApi, License, LicenseClass,
    Manifest, PathStyle, Severity, extension::MANIFEST_FILE, load_manifest, platform::Os,
};
use serde_json::json;

/// A notes app at extension API 1.3, with two kinds of its own.
fn api() -> ExtensionApi {
    ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 3 },
        reserved: vec!["app".into(), "editor".into()],
        kinds: vec![
            AppKind {
                name: "lsp",
                schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "args": { "type": "array" },
                    },
                }),
            },
            AppKind {
                name: "grammars",
                schema: json!({ "type": "object" }),
            },
        ],
        allowed_licenses: None,
    }
}

const VALID: &str = r#"id = "acme.solarized"
name = "Solarized"
version = "1.2.3"
description = "A calm palette"
license = "MIT"

[engine]
notes = "1.2"

[contributes]
themes = ["themes/dark.toml", "./themes/light.toml"]
keymaps = ["keymaps/vim.toml"]
menus = ["menus.toml"]

[contributes.lsp]
command = "solarized-ls"
args = ["--stdio"]
"#;

/// An extension directory holding `manifest` and the files [`VALID`]
/// contributes.
fn extension(manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in [
        "themes/dark.toml",
        "themes/light.toml",
        "keymaps/vim.toml",
        "menus.toml",
    ] {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    fs::write(dir.path().join(MANIFEST_FILE), manifest).unwrap();
    dir
}

fn load(manifest: &str) -> (tempfile::TempDir, Option<Manifest>, Vec<Diagnostic>) {
    load_with(manifest, &api())
}

fn load_with(
    manifest: &str,
    api: &ExtensionApi,
) -> (tempfile::TempDir, Option<Manifest>, Vec<Diagnostic>) {
    let dir = extension(manifest);
    let (loaded, diagnostics) = load_manifest(dir.path(), api);
    (dir, loaded, diagnostics)
}

/// The 1-based line and column of the first `needle` in `source`.
fn position(source: &str, needle: &str) -> (usize, usize) {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} isn't in the manifest"));
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap().chars().count() + 1;
    (line, column)
}

/// Asserts that loading `manifest` refuses it with an error naming the
/// manifest, `key`, and the position of `token` (or 1:1 for `None`).
fn assert_refused(manifest: &str, key: &str, token: Option<&str>) -> Diagnostic {
    let (dir, loaded, diagnostics) = load(manifest);
    assert!(loaded.is_none(), "{key}: loaded {loaded:?}");
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error() && diagnostic.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no error at {key}: {diagnostics:#?}"))
        .clone();
    assert_eq!(error.file, dir.path().join(MANIFEST_FILE), "{key}");
    let expected = token.map_or((1, 1), |token| position(manifest, token));
    assert_eq!((error.line, error.column), expected, "{key}: {error}");
    error
}

#[test]
fn a_valid_manifest_loads() {
    let (dir, loaded, diagnostics) = load(VALID);
    assert_eq!(diagnostics, []);
    let manifest = loaded.unwrap();
    let under = |file: &str| dir.path().join(file);
    assert_eq!(manifest.dir, dir.path());
    assert_eq!(manifest.id, "acme.solarized");
    assert_eq!(manifest.name, "Solarized");
    assert_eq!(manifest.version, (1, 2, 3));
    assert_eq!(manifest.description.as_deref(), Some("A calm palette"));
    assert_eq!(
        manifest.license,
        License {
            expr: "MIT".into(),
            class: LicenseClass::Permissive
        }
    );
    assert_eq!(manifest.engine, ApiVersion { major: 1, minor: 2 });
    assert_eq!(
        manifest.themes,
        [under("themes/dark.toml"), under("themes/light.toml")]
    );
    assert_eq!(manifest.keymaps, [under("keymaps/vim.toml")]);
    assert_eq!(manifest.menus, [under("menus.toml")]);
    assert!(manifest.themes.iter().all(|path| path.is_absolute()));
    let lsp: toml::Value = toml::from_str(
        r#"
        command = "solarized-ls"
        args = ["--stdio"]
        "#,
    )
    .unwrap();
    assert_eq!(manifest.app.len(), 1);
    assert_eq!(manifest.app["lsp"], lsp);
}

#[test]
fn a_manifest_with_no_contributions_loads() {
    let manifest = "id = \"acme.empty\"\nname = \"Empty\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n[engine]\nnotes = \"1.0\"\n";
    let (_dir, loaded, diagnostics) = load(manifest);
    assert_eq!(diagnostics, []);
    let loaded = loaded.unwrap();
    assert!(loaded.themes.is_empty() && loaded.app.is_empty());
    assert_eq!(loaded.description, None);
}

#[test]
fn manifest_errors_name_their_key_and_position() {
    let with = |from: &str, to: &str| {
        assert!(VALID.contains(from), "{from}");
        VALID.replacen(from, to, 1)
    };

    let manifest = with("\"acme.solarized\"", "\"Solarized\"");
    let error = assert_refused(&manifest, "id", Some("\"Solarized\""));
    assert!(error.message.contains("isn't an extension ID"), "{error}");
    let manifest = with("\"acme.solarized\"", "\"solarized\"");
    assert_refused(&manifest, "id", Some("\"solarized\""));

    let manifest = with("\"acme.solarized\"", "\"editor.solarized\"");
    let error = assert_refused(&manifest, "id", Some("\"editor.solarized\""));
    assert!(error.message.contains("reserved"), "{error}");

    let manifest = with("\"1.2.3\"", "\"1.2\"");
    assert_refused(&manifest, "version", Some("\"1.2\""));
    let manifest = with("\"1.2.3\"", "\"1.2.3-beta\"");
    assert_refused(&manifest, "version", Some("\"1.2.3-beta\""));
    let manifest = with("\"1.2.3\"", "\"1.+2.3\"");
    assert_refused(&manifest, "version", Some("\"1.+2.3\""));

    let manifest = with("[engine]\nnotes = \"1.2\"\n", "");
    let error = assert_refused(&manifest, "engine", None);
    assert!(error.message.contains("missing [engine]"), "{error}");

    let manifest = with("name = \"Solarized\"\n", "");
    assert_refused(&manifest, "name", None);

    let absolute = if cfg!(windows) {
        "C:\\\\themes\\\\dark.toml"
    } else {
        "/themes/dark.toml"
    };
    let manifest = with("\"./themes/light.toml\"", &format!("\"{absolute}\""));
    let error = assert_refused(
        &manifest,
        "contributes.themes",
        Some(&format!("\"{absolute}\"")),
    );
    assert!(error.message.contains("must be relative"), "{error}");

    let manifest = with("\"keymaps/vim.toml\"", "\"keymaps/../../vim.toml\"");
    let error = assert_refused(
        &manifest,
        "contributes.keymaps",
        Some("\"keymaps/../../vim.toml\""),
    );
    assert!(error.message.contains("outside"), "{error}");

    // A file not there yet is its reader's to report: the rest loads.
    let manifest = with("\"menus.toml\"", "\"missing.toml\"");
    let (dir, loaded, diagnostics) = load(&manifest);
    assert_eq!(diagnostics, []);
    assert_eq!(loaded.unwrap().menus, [dir.path().join("missing.toml")]);

    // A missing directory before `..` is missing, as the OS resolves it.
    if cfg!(unix) {
        let manifest = with("\"menus.toml\"", "\"missing/../menus.toml\"");
        let error = assert_refused(
            &manifest,
            "contributes.menus",
            Some("\"missing/../menus.toml\""),
        );
        assert!(error.message.contains("doesn't exist"), "{error}");
    }

    // `..` that stays inside is fine.
    let manifest = with("\"menus.toml\"", "\"themes/../menus.toml\"");
    let (dir, loaded, diagnostics) = load(&manifest);
    assert_eq!(diagnostics, []);
    assert_eq!(loaded.unwrap().menus, [dir.path().join("menus.toml")]);
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_directory_is_refused() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("stolen.toml"), "").unwrap();
    let manifest = VALID.replacen("\"menus.toml\"", "\"linked.toml\"", 1);
    let dir = extension(&manifest);
    std::os::unix::fs::symlink(
        outside.path().join("stolen.toml"),
        dir.path().join("linked.toml"),
    )
    .unwrap();
    let (loaded, diagnostics) = load_manifest(dir.path(), &api());
    assert!(loaded.is_none());
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.key.as_deref() == Some("contributes.menus"))
        .unwrap();
    assert!(error.is_error());
    assert_eq!(error.file, dir.path().join(MANIFEST_FILE));
    assert_eq!(
        (error.line, error.column),
        position(&manifest, "\"linked.toml\"")
    );
    assert!(error.message.contains("outside"), "{error}");

    // A link to a file inside the directory is fine, and is kept as the
    // file it leads to.
    fs::remove_file(dir.path().join("linked.toml")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("menus.toml"),
        dir.path().join("linked.toml"),
    )
    .unwrap();
    let (loaded, diagnostics) = load_manifest(dir.path(), &api());
    assert_eq!(diagnostics, []);
    assert_eq!(loaded.unwrap().menus, [dir.path().join("menus.toml")]);

    // `..` after a link climbs from where the link leads, not from the link.
    fs::create_dir(outside.path().join("sub")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("sub"), dir.path().join("away")).unwrap();
    fs::write(outside.path().join("menus.toml"), "").unwrap();
    let manifest = VALID.replacen("\"menus.toml\"", "\"away/../menus.toml\"", 1);
    fs::write(dir.path().join(MANIFEST_FILE), &manifest).unwrap();
    let (loaded, diagnostics) = load_manifest(dir.path(), &api());
    assert!(loaded.is_none());
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.key.as_deref() == Some("contributes.menus"))
        .unwrap();
    assert!(error.message.contains("outside"), "{error}");
    assert_eq!(
        (error.line, error.column),
        position(&manifest, "\"away/../menus.toml\"")
    );

    // So is a missing file through a link out.
    let manifest = VALID.replacen("\"menus.toml\"", "\"away/missing.toml\"", 1);
    fs::write(dir.path().join(MANIFEST_FILE), &manifest).unwrap();
    let (loaded, diagnostics) = load_manifest(dir.path(), &api());
    assert!(loaded.is_none());
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.key.as_deref() == Some("contributes.menus"))
        .unwrap();
    assert!(error.message.contains("outside"), "{error}");
}

#[test]
fn a_missing_or_broken_manifest_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let (loaded, diagnostics) = load_manifest(dir.path(), &api());
    assert!(loaded.is_none());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].is_error());
    assert_eq!(diagnostics[0].file, dir.path().join(MANIFEST_FILE));

    let (_dir, loaded, diagnostics) = load("id = \n");
    assert!(loaded.is_none());
    assert!(diagnostics[0].is_error(), "{diagnostics:?}");

    let (_dir, loaded, diagnostics) = load("");
    assert!(loaded.is_none());
    assert!(diagnostics.iter().any(Diagnostic::is_error));
}

#[test]
fn the_engine_must_be_this_app_and_api() {
    let (_dir, loaded, diagnostics) = load(VALID);
    assert_eq!(diagnostics, []);
    assert_eq!(loaded.unwrap().engine, ApiVersion { major: 1, minor: 2 });
    let exact = VALID.replacen("notes = \"1.2\"", "notes = \"1.3\"", 1);
    assert!(load(&exact).2.is_empty());

    for needed in ["1.4", "2.0"] {
        let manifest = VALID.replacen("\"1.2\"", &format!("\"{needed}\""), 1);
        let error = assert_refused(&manifest, "engine.notes", Some(&format!("\"{needed}\"")));
        assert_eq!(
            error.message,
            format!("needs extension API {needed}; this app has 1.3")
        );
    }

    let manifest = VALID.replacen("notes = \"1.2\"", "tasks = \"1.2\"", 1);
    let error = assert_refused(&manifest, "engine.tasks", Some("tasks"));
    assert!(
        error.message.contains("is for tasks, not this app"),
        "{error}"
    );

    let manifest = VALID.replacen("\"1.2\"", "\"1\"", 1);
    assert_refused(&manifest, "engine.notes", Some("\"1\""));
}

#[test]
fn plugins_and_unknown_kinds() {
    let manifest = format!("{VALID}\n[plugin]\nwasm = \"plugin.wasm\"\n");
    let error = assert_refused(&manifest, "plugin", Some("plugin]"));
    assert_eq!(error.message, "code plugins aren't supported by this app");

    // An unknown kind is ignored, with a warning.
    let manifest = format!("{VALID}\n[contributes.snippets]\nrust = \"rust.toml\"\n");
    let (_dir, loaded, diagnostics) = load(&manifest);
    let loaded = loaded.expect("loads without the unknown kind");
    assert!(!loaded.app.contains_key("snippets"));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let warning = &diagnostics[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.key.as_deref(), Some("contributes.snippets"));
    assert_eq!(
        (warning.line, warning.column),
        position(&manifest, "snippets")
    );

    // So is an unknown top-level key.
    let manifest = format!("homepage = \"https://example.com\"\n{VALID}");
    let (_dir, loaded, diagnostics) = load(&manifest);
    assert!(loaded.is_some());
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].key.as_deref(), Some("homepage"));

    // An app kind that breaks its schema is an error at the field.
    let manifest = VALID.replacen("command = \"solarized-ls\"", "command = 5", 1);
    let error = assert_refused(&manifest, "contributes.lsp.command", Some("5\n"));
    assert!(error.message.contains("expected a string"), "{error}");

    // Grammars, like every extension, need a licence.
    let manifest = format!("{VALID}\n[contributes.grammars]\nzig = \"grammars/zig.wasm\"\n")
        .replacen("license = \"MIT\"\n", "", 1);
    let error = assert_refused(&manifest, "license", None);
    assert!(error.message.contains("missing `license`"), "{error}");
}

#[test]
fn too_many_contributed_files_is_an_error() {
    let entries = vec!["\"menus.toml\""; kiln_core::extension::MAX_CONTRIBUTED_FILES];
    let manifest = VALID.replacen(
        "menus = [\"menus.toml\"]",
        &format!("menus = [{}]", entries.join(", ")),
        1,
    );
    assert_refused(&manifest, "contributes", Some("contributes]"));
}

#[test]
fn a_huge_contributed_array_is_refused_before_its_entries_are_checked() {
    let entries = vec!["5"; 200_000];
    let manifest = VALID.replacen(
        "menus = [\"menus.toml\"]",
        &format!("menus = [{}]", entries.join(",")),
        1,
    );
    let (_dir, loaded, diagnostics) = load(&manifest);
    assert!(loaded.is_none());
    assert_eq!(
        diagnostics.len(),
        1,
        "{:#?}",
        &diagnostics[..diagnostics.len().min(3)]
    );
    assert_eq!(diagnostics[0].key.as_deref(), Some("contributes"));
    assert!(
        diagnostics[0].message.contains("200003 files"),
        "{}",
        diagnostics[0]
    );
}

#[test]
fn app_kind_schemas_keep_their_definitions() {
    let mut api = api();
    api.kinds = vec![
        AppKind {
            name: "lsp",
            schema: json!({
                "type": "object",
                "properties": { "server": { "$ref": "#/$defs/Server" } },
                "$defs": {
                    "Server": {
                        "type": "object",
                        "properties": { "command": { "$ref": "#/$defs/Command" } },
                    },
                    "Command": { "type": "string" },
                },
            }),
        },
        AppKind {
            name: "tasks",
            schema: json!({
                "type": "object",
                "properties": { "server": { "$ref": "#/$defs/Server" } },
                "$defs": { "Server": { "type": "integer" } },
            }),
        },
    ];
    let base = VALID.replacen(
        "[contributes.lsp]\ncommand = \"solarized-ls\"\nargs = [\"--stdio\"]\n",
        "[contributes.lsp.server]\ncommand = \"solarized-ls\"\n[contributes.tasks]\nserver = 3\n",
        1,
    );
    let (_dir, loaded, diagnostics) = load_with(&base, &api);
    assert_eq!(diagnostics, []);
    assert_eq!(loaded.unwrap().app.len(), 2);

    let manifest = base.replacen("command = \"solarized-ls\"", "command = 5", 1);
    let (dir, loaded, diagnostics) = load_with(&manifest, &api);
    assert!(loaded.is_none());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let error = &diagnostics[0];
    assert_eq!(error.key.as_deref(), Some("contributes.lsp.server.command"));
    assert_eq!(error.file, dir.path().join(MANIFEST_FILE));
    assert_eq!((error.line, error.column), position(&manifest, "5\n"));

    let manifest = base.replacen("server = 3", "server = \"three\"", 1);
    let (_dir, loaded, diagnostics) = load_with(&manifest, &api);
    assert!(loaded.is_none());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(
        diagnostics[0].key.as_deref(),
        Some("contributes.tasks.server")
    );
}

#[test]
fn licence_expressions_are_bounded() {
    let deep = format!("{}MIT{}", "(".repeat(100_000), ")".repeat(100_000));
    assert!(License::parse(&deep).is_err());
    let (_dir, loaded, diagnostics) = load(&licensed(&deep));
    assert!(loaded.is_none());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.key.as_deref() == Some("license"))
    );

    let nested = |depth: usize| format!("{}MIT{}", "(".repeat(depth), ")".repeat(depth));
    assert!(License::parse(&nested(16)).is_ok());
    assert!(License::parse(&nested(17)).is_err());

    let long = vec!["MIT"; 100_000].join(" AND ");
    assert!(License::parse(&long).is_err());
    let flat = vec!["MIT"; 140].join(" OR ");
    assert_eq!(
        License::parse(&flat).map(|license| license.class),
        Ok(LicenseClass::Permissive)
    );
}

fn licensed(expr: &str) -> String {
    VALID.replacen("license = \"MIT\"", &format!("license = {expr:?}"), 1)
}

#[test]
fn extension_licence_handling() {
    let manifest = VALID.replacen("license = \"MIT\"\n", "", 1);
    let error = assert_refused(&manifest, "license", None);
    assert!(error.message.contains("license"), "{error}");

    let manifest = licensed("MIT AND (");
    let error = assert_refused(&manifest, "license", Some("\"MIT AND (\""));
    assert!(error.message.contains("isn't an SPDX"), "{error}");

    for (expr, class) in [
        ("MIT", LicenseClass::Permissive),
        ("Apache-2.0 OR MIT", LicenseClass::Permissive),
        ("GPL-3.0-only", LicenseClass::Copyleft),
        ("MIT AND GPL-2.0-only", LicenseClass::Copyleft),
        ("LicenseRef-acme", LicenseClass::Unknown),
        ("GPL-2.0-only OR MIT", LicenseClass::Permissive),
        ("NOASSERTION", LicenseClass::Unknown),
        (
            "(MIT OR GPL-2.0+) AND Apache-2.0 WITH LLVM-exception",
            LicenseClass::Permissive,
        ),
        ("MIT AND LicenseRef-acme", LicenseClass::Unknown),
        (
            "GPL-2.0-or-later OR LicenseRef-acme",
            LicenseClass::Copyleft,
        ),
    ] {
        assert_eq!(
            License::parse(expr).map(|license| license.class),
            Ok(class),
            "{expr}"
        );
        let (_dir, loaded, diagnostics) = load(&licensed(expr));
        assert_eq!(diagnostics, [], "{expr}");
        assert_eq!(loaded.unwrap().license.class, class, "{expr}");
    }
    for bad in [
        "",
        "MIT AND",
        "MIT OR OR MIT",
        "(MIT",
        "MIT)",
        "MIT and Apache-2.0",
        "MIT WITH",
        "MIT/Apache-2.0",
        ":",
        "LicenseRef-",
        "MIT WITH :",
        "MIT +",
        "+",
        "LicenseRef-acme+",
        "DocumentRef-:LicenseRef-acme",
        "DocumentRef-a:MIT",
        "MIT:Apache-2.0",
        "MIT WITH LLVM-exception+",
        "GPL-2.0+OR MIT",
        "MIT+AND Apache-2.0",
        "GPL-2.0+WITH Classpath-exception-2.0",
        "GPL-2.0++",
    ] {
        assert!(License::parse(bad).is_err(), "{bad:?}");
        let manifest = licensed(bad);
        let (_dir, loaded, diagnostics) = load(&manifest);
        assert!(loaded.is_none(), "{bad:?}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.is_error()
                    && diagnostic.key.as_deref() == Some("license")),
            "{bad:?}: {diagnostics:#?}"
        );
    }
    for (good, class) in [
        ("GPL-2.0+", LicenseClass::Copyleft),
        (
            "DocumentRef-spdx-tool-1.2:LicenseRef-MIT-Style-2",
            LicenseClass::Unknown,
        ),
        ("Apache-2.0 WITH LLVM-exception", LicenseClass::Permissive),
        (
            "GPL-2.0-only WITH Classpath-exception-2.0",
            LicenseClass::Copyleft,
        ),
        ("((MIT))", LicenseClass::Permissive),
        ("(GPL-2.0+)AND(MIT)", LicenseClass::Copyleft),
        ("GPL-2.0+ OR MIT", LicenseClass::Permissive),
    ] {
        assert_eq!(
            License::parse(good).map(|license| license.class),
            Ok(class),
            "{good}"
        );
    }

    let mut only_mit = api();
    only_mit.allowed_licenses = Some(vec!["MIT".into()]);
    let manifest = licensed("GPL-3.0-only");
    let (_dir, loaded, diagnostics) = load_with(&manifest, &only_mit);
    assert!(loaded.is_none());
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
        .unwrap();
    assert_eq!(error.key.as_deref(), Some("license"));
    assert_eq!(
        error.message,
        "acme.solarized: licence GPL-3.0-only isn't in extensions.allowed_licenses"
    );
    assert_eq!(
        (error.line, error.column),
        position(&manifest, "\"GPL-3.0-only\"")
    );

    // Not read as `GPL-2.0+ OR MIT`, which MIT would allow.
    let (_dir, loaded, _) = load_with(&licensed("GPL-2.0+OR MIT"), &only_mit);
    assert!(loaded.is_none());

    let (_dir, loaded, diagnostics) = load_with(&licensed("Apache-2.0 OR MIT"), &only_mit);
    assert_eq!(diagnostics, []);
    assert!(loaded.is_some());
    let (_dir, loaded, _) = load_with(&licensed("Apache-2.0 AND MIT"), &only_mit);
    assert!(loaded.is_none());
}

#[test]
fn extensions_live_in_the_state_dir() {
    let id = AppId::new("com.example.notes", "notes", "Notes");
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    let home: &[(&str, &str)] = &[("HOME", "/home/u")];
    let paths = AppPaths::resolve(&id, PathStyle::Native, Os::Linux, env(home));
    assert_eq!(
        paths.extensions_dir(),
        Some(PathBuf::from("/home/u/.local/state/notes/extensions"))
    );
    for os in [Os::Linux, Os::MacOs, Os::Windows] {
        let vars: &[(&str, &str)] = &[
            ("HOME", "/home/u"),
            ("APPDATA", "/appdata"),
            ("LOCALAPPDATA", "/local"),
        ];
        let paths = AppPaths::resolve(&id, PathStyle::Native, os, env(vars));
        assert_eq!(
            paths.extensions_dir(),
            paths.state_dir().map(|dir| dir.join("extensions")),
            "{os:?}"
        );
    }
    assert_eq!(
        paths.project_extensions_dir(Path::new("/work")),
        Path::new("/work/.notes/extensions")
    );
}
