//! Tenmoku's own code is Apache-2.0: the licence files and manifests say so, and
//! every bundled third-party theme is credited.

use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn license_is_the_apache_text() {
    assert_eq!(read("LICENSE"), read("LICENSES/Apache-2.0.txt"));
}

#[test]
fn manifests_say_apache() {
    let workspace: toml::Table = read("Cargo.toml").parse().unwrap();
    assert_eq!(
        workspace["workspace"]["package"]["license"].as_str(),
        Some("Apache-2.0")
    );
    let fuzz: toml::Table = read("fuzz/Cargo.toml").parse().unwrap();
    assert_eq!(fuzz["package"]["license"].as_str(), Some("Apache-2.0"));
}

#[test]
fn notice_reserves_nothing() {
    let notice = read("NOTICE");
    assert!(notice.starts_with("Tenmoku\n"));
    assert!(!notice.contains("All rights reserved"));
}

/// Bundled theme files written for Tenmoku without a `tenmoku-` name, so
/// nothing to credit.
const FIRST_PARTY: &[&str] = &["yohen.toml", "hakuji.toml"];

#[test]
fn third_party_themes_are_credited() {
    let (notice, third_party) = (read("NOTICE"), read("THIRD_PARTY.md"));
    let dir = root().join("crates/fe-config/themes");
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.starts_with("tenmoku-") {
            continue;
        }
        if FIRST_PARTY.contains(&name.as_str()) {
            continue;
        }
        assert!(notice.contains(&name), "{name} is not in NOTICE");
        assert!(
            third_party.contains(&name),
            "{name} is not in THIRD_PARTY.md"
        );
    }
}
