//! Every first-party grammar directory has its wasm recorded in
//! `extensions/grammars/pack.toml`, so the app can fetch it.

use std::{fs, path::Path};

/// Directories with no recorded entry yet, each with why.
const NOT_RELEASED: &[(&str, &str)] = &[];

fn table(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    toml::from_str(&text).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

#[test]
fn every_grammar_directory_has_a_recorded_pack_entry() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let grammars = Path::new(&manifest).join("../../extensions/grammars");
    let pack = table(&grammars.join("pack.toml"));
    let entries = pack
        .get("languages")
        .and_then(|entries| entries.as_table())
        .expect("pack.toml has [languages.*] entries");

    let mut checked = 0;
    for dir in fs::read_dir(&grammars).unwrap() {
        let dir = dir.unwrap();
        if !dir.file_type().unwrap().is_dir() {
            continue;
        }
        let language = dir.file_name().into_string().unwrap();
        if NOT_RELEASED.iter().any(|(name, _)| *name == language) {
            continue;
        }
        let extension = table(&dir.path().join("extension.toml"));
        let wasm = extension["contributes"]["grammars"][0]["wasm"]
            .as_str()
            .unwrap_or_else(|| panic!("{language}: extension.toml names no wasm"));
        let entry = entries
            .get(&language)
            .and_then(|entry| entry.as_table())
            .unwrap_or_else(|| panic!("pack.toml has no [languages.{language}]"));
        assert_eq!(entry["file"].as_str(), Some(wasm), "{language}'s file");
        let size = entry["size"].as_integer().unwrap();
        assert!(
            size > 0 && size as u64 <= fe_syntax::MAX_GRAMMAR_BYTES,
            "{language}'s size {size}"
        );
        let sha256 = entry["sha256"].as_str().unwrap();
        assert!(
            sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "{language}'s sha256 {sha256:?}"
        );
        checked += 1;
    }
    assert!(checked > 0);
}
