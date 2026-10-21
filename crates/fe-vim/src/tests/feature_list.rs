//! `FEATURES` lists every module that defines a `FEATURE`, each once, so
//! every feature's hooks and commands are reached and none runs twice.

use std::path::{Path, PathBuf};

use crate::feature::FEATURES;

const DEFINITION: &str = "pub(crate) const FEATURE: Feature";

/// The stems of the files under `dir` that define a `FEATURE`.
fn defined(dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            defined(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source = std::fs::read_to_string(&path).expect("read");
            if source.lines().any(|line| line.starts_with(DEFINITION)) {
                let stem = path.file_stem().expect("stem").to_string_lossy();
                found.push(stem.into_owned());
            }
        }
    }
}

#[test]
fn features_list_every_feature_module_once() {
    // At run time, not `env!`: a shared target can keep another checkout's path.
    let src = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| env!("CARGO_MANIFEST_DIR").into(), PathBuf::from)
        .join("src");
    let mut modules = Vec::new();
    defined(&src, &mut modules);
    modules.sort();

    let hub = std::fs::read_to_string(src.join("feature.rs")).expect("feature.rs");
    let list = hub
        .split_once("const FEATURES: &[&Feature] = &[")
        .and_then(|(_, rest)| rest.split_once("];"))
        .expect("the FEATURES list")
        .0;
    let mut listed: Vec<String> = list
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let path = entry
                .strip_prefix('&')
                .and_then(|e| e.strip_suffix("::FEATURE"));
            let path = path.unwrap_or_else(|| panic!("{entry:?} isn't a module's FEATURE"));
            path.rsplit("::").next().unwrap().to_owned()
        })
        .collect();
    assert_eq!(listed.len(), FEATURES.len(), "the list read is FEATURES");
    listed.sort();
    assert_eq!(listed, modules);
}
