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

use kiln_l10n::{Args, Catalogue, Resources, lint};

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

/// Sample argument sets: a count that takes a plural's `one`, one that takes
/// `other`, and a string, so each branch of a selector is formatted.
fn sample_args(variables: &BTreeSet<String>) -> Vec<Args> {
    let numbers = |n: i64| {
        variables
            .iter()
            .fold(Args::new(), |args, name| args.with(name.clone(), n))
    };
    let strings = variables
        .iter()
        .fold(Args::new(), |args, name| args.with(name.clone(), "sample"));
    vec![numbers(1), numbers(5), strings]
}

#[test]
fn every_message_formats_in_the_pseudo_locale() {
    let registered = Resources::registered();
    let pseudo: kiln_l10n::LanguageIdentifier = kiln_l10n::PSEUDO_LOCALE.parse().unwrap();
    let catalogue = Catalogue::new(&registered, std::slice::from_ref(&pseudo));
    assert_eq!(catalogue.errors(), [] as [String; 0]);
    assert_eq!(catalogue.chain()[0], pseudo, "en-XA is built on request");

    let mut formatted = 0;
    let mut failures = Vec::new();
    for resources in &registered {
        for file in resources.files.iter().filter(|f| f.locale == "en-US") {
            for shape in kiln_l10n::message_shapes(file.source) {
                for args in sample_args(&shape.variables) {
                    let mut results = Vec::new();
                    if shape.has_value {
                        results.push(catalogue.try_format(&shape.key, &args));
                    }
                    for name in &shape.attributes {
                        results.push(catalogue.try_attribute(&shape.key, name, &args));
                    }
                    for result in results {
                        formatted += 1;
                        match result {
                            Some(f) if f.errors.is_empty() && f.locale == pseudo => {
                                assert!(
                                    f.text.starts_with('⟦') && f.text.ends_with('⟧'),
                                    "{}: {}",
                                    shape.key,
                                    f.text
                                );
                            }
                            Some(f) => failures.push(format!(
                                "{} ({}): {} {:?}",
                                shape.key, resources.crate_name, f.locale, f.errors
                            )),
                            None => failures.push(format!("{}: not found", shape.key)),
                        }
                    }
                }
            }
        }
    }
    assert!(formatted > 100, "formatted only {formatted} messages");
    assert_eq!(failures, [] as [String; 0], "messages that fail in en-XA");
}

#[test]
fn the_pseudo_locale_is_not_shipped_or_in_the_schema() {
    let shipped = kiln_l10n::shipped_locales();
    assert!(
        shipped.iter().all(|l| *l != kiln_l10n::PSEUDO_LOCALE),
        "{shipped:?}"
    );
    let schema = std::fs::read_to_string(root().join("crates/fe-config/settings.schema.json"))
        .unwrap()
        .to_lowercase();
    assert!(
        !schema.contains("en-xa"),
        "the schema names the pseudo-locale"
    );
    for resources in Resources::registered() {
        assert!(
            !resources.locales().contains(kiln_l10n::PSEUDO_LOCALE),
            "{} ships en-XA files",
            resources.crate_name
        );
    }
}

#[test]
fn the_pseudo_locale_is_selected_only_by_the_environment() {
    let ids = |tags: &[&str]| -> Vec<kiln_l10n::LanguageIdentifier> {
        tags.iter().map(|t| t.parse().unwrap()).collect()
    };
    // A setting or a system language of en-XA is dropped without
    // TENMOKU_PSEUDO_LOCALE=1; with it, en-XA leads an explicit en-US.
    let asked = ids(&["en-US", "en-XA", "de"]);
    assert_eq!(
        kiln_platform::locale::gate_pseudo(&asked, false),
        ids(&["en-US", "de"])
    );
    assert_eq!(
        kiln_platform::locale::gate_pseudo(&asked, true),
        ids(&["en-XA", "en-US", "de"])
    );
}
