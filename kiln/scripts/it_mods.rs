// SPDX-License-Identifier: LicenseRef-Proprietary

// The build script of a kiln crate whose integration tests are one binary,
// tests/it.rs (TESTING.md, "Integration tests"), include!d by the crate's
// build.rs. Cargo links each tests/*.rs into its own executable, a copy of
// GPUI in each; this writes `$OUT_DIR/it_mods.rs` instead, which tests/it.rs
// include!s:
//
// - one `#[path = "<absolute path>"] mod <stem>;` per tests/*.rs but it.rs,
//   sorted, so a test's ID is `<stem>::<test>`. Subdirectories, such as
//   tests/support, aren't listed: the files that use them declare them.
// - `IT_MODS`, the stems, for tests/it.rs's check that no file is left out.
//
// Adding a file reruns it (the directory is watched), so a new test is a new
// file and nothing else; no list is kept by hand. A proc macro couldn't do
// this: cargo doesn't rerun one when a file appears.

use std::{env, fs, path::PathBuf};

fn main() {
    let tests =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo")).join("tests");
    println!("cargo:rerun-if-changed={}", tests.display());

    let mut files = Vec::new();
    for entry in fs::read_dir(&tests).unwrap_or_else(|error| panic!("{}: {error}", tests.display()))
    {
        let path = entry.expect("a tests/ entry").path();
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_owned);
        let stem = stem.unwrap_or_else(|| panic!("{}: not a UTF-8 name", path.display()));
        if stem == "it" {
            continue;
        }
        let ident = stem.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            && stem
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        assert!(
            ident,
            "{}: name a test file in snake_case, as its module is",
            path.display()
        );
        files.push((stem, path));
    }
    files.sort();

    let mut out = String::new();
    for (stem, path) in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let path = path
            .to_str()
            .unwrap_or_else(|| panic!("{}: not a UTF-8 path", path.display()));
        out += &format!("#[path = {path:?}]\nmod {stem};\n");
    }
    let stems: Vec<String> = files.iter().map(|(stem, _)| format!("{stem:?}")).collect();
    out += &format!(
        "\n/// tests/*.rs but it.rs, as they were when this was generated.\nconst IT_MODS: &[&str] = &[{}];\n",
        stems.join(", ")
    );

    // Written only when the list changes: an edited test leaves its mtime be.
    let target = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo")).join("it_mods.rs");
    if fs::read_to_string(&target).ok().as_deref() != Some(out.as_str()) {
        fs::write(&target, out).unwrap_or_else(|error| panic!("{}: {error}", target.display()));
    }
}
