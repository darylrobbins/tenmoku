//! The app's string catalogue, at its root (i18n design §§ 3 and 8).
//!
//! - `catalogue_loads_without_duplicates`: every crate's resources the app
//!   links load into one catalogue per shipped locale with no parse error
//!   and no key defined twice, and the crates linked are every crate that
//!   ships an `l10n/` directory, so none is left out of the app.
//! - `app_catalogue_is_complete`: the whole tree, read from disk (every
//!   `.ftl` under `crates/*/l10n/` and `kiln/crates/*/l10n/`, every non-test
//!   source file and the bundled menus' `title_key`s), passes the root
//!   checks: no unused key, every reference resolves in its own locale in
//!   every branch, every message formats, and translations stay in step with
//!   en-US. None is exempt, not even kiln's `## public` keys: the app sees
//!   every user of them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kiln_l10n::{Catalogue, Resources, lint};

// Linked, so the resources of every crate they reach are registered, as the
// app's binary links them.
use fe_buffer as _;
use fe_config as _;
use fe_gpui as _;
use fe_lsp as _;
use fe_net as _;
use fe_syntax as _;
use fe_vim as _;
use fe_workbench as _;
use kiln as _;
use kiln_platform as _;

/// The repository's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The crates directly under `dir` that ship an `l10n/` directory.
fn crates_with_catalogues(dir: &Path) -> BTreeSet<String> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file() && path.join("l10n").is_dir())
        .map(|path| package_name(&path))
        .collect()
}

/// The `[package] name` of the crate at `dir`.
fn package_name(dir: &Path) -> String {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let manifest: toml::Table = toml::from_str(&manifest).unwrap();
    manifest["package"]["name"].as_str().unwrap().to_owned()
}

#[test]
fn catalogue_loads_without_duplicates() {
    let registered = Resources::registered();
    let linked: BTreeSet<String> = registered.iter().map(|r| r.crate_name.to_owned()).collect();
    let mut on_disk = crates_with_catalogues(&root().join("crates"));
    on_disk.extend(crates_with_catalogues(&root().join("kiln/crates")));
    assert!(
        on_disk.contains("fe-config"),
        "fe-config ships a catalogue: {on_disk:?}"
    );
    assert_eq!(
        linked, on_disk,
        "every crate with a catalogue is linked into the app, and only those"
    );

    let shipped = kiln_l10n::shipped_locales();
    assert!(shipped.iter().any(|locale| *locale == "en-US"));
    for locale in &shipped {
        let catalogue = Catalogue::new(&registered, std::slice::from_ref(locale));
        assert_eq!(
            catalogue.errors(),
            [] as [String; 0],
            "{locale}'s bundles parse, with no key defined twice"
        );
        assert_eq!(catalogue.chain()[0], *locale);
    }
}

/// Every `title_key` in a bundled menu file.
fn menu_title_keys() -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("crates/fe-keymap/menus"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.push(root().join("kiln/crates/kiln/src/setup/menus.toml"));
    let mut keys = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let table: toml::Table =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        title_keys(&toml::Value::Table(table), &mut keys);
    }
    keys
}

fn title_keys(value: &toml::Value, keys: &mut Vec<String>) {
    match value {
        toml::Value::Table(table) => {
            for (name, value) in table {
                match value {
                    toml::Value::String(key) if name == "title_key" => keys.push(key.clone()),
                    _ => title_keys(value, keys),
                }
            }
        }
        toml::Value::Array(values) => values.iter().for_each(|value| title_keys(value, keys)),
        _ => {}
    }
}

#[test]
fn app_catalogue_is_complete() {
    lint::catalogue()
        .crates_under(root().join("crates"))
        .crates_under(root().join("kiln/crates"))
        .data_keys(menu_title_keys())
        .check();
}
