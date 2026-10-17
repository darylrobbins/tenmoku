#![cfg(feature = "json")]

use std::path::Path;

use fe_syntax::language_name_for_path;

#[test]
fn vs_code_json_extensions_and_names_are_json() {
    for name in [
        "x.jsonc",
        "app.webmanifest",
        "trace.har",
        "x.jsonld",
        "map.geojson",
        "notes.ipynb",
        "X.IPYNB",
        "composer.lock",
        "bun.lock",
        ".babelrc",
        ".eslintrc",
        ".jshintrc",
        ".swcrc",
        ".watchmanconfig",
        ".ember-cli",
        "a.babelrc",
    ] {
        assert_eq!(
            language_name_for_path(&Path::new("proj").join(name)),
            Some("json"),
            "{name}"
        );
    }
    assert_eq!(language_name_for_path(Path::new("proj/yarn.lock")), None);
    assert_eq!(language_name_for_path(Path::new("proj/x.jsonl")), None);
}

#[cfg(feature = "toml")]
#[test]
fn cargo_lock_stays_toml() {
    assert_eq!(
        language_name_for_path(Path::new("proj/Cargo.lock")),
        Some("toml")
    );
}
