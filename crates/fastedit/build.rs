//! Compiles in each first-party grammar's extension files: a directory under
//! `extensions/grammars/` is a language (`src/grammar_install.rs`), so a new
//! one needs no source edit. Writes `$OUT_DIR/first_party.rs`, the table
//! `FIRST_PARTY` of `(language, ExtensionFiles)` sorted by language.

use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let grammars = manifest_dir.join("../../extensions/grammars");
    println!("cargo:rerun-if-changed=build.rs");
    // A directory added or removed; include_str! tracks the files themselves.
    println!("cargo:rerun-if-changed={}", grammars.display());

    let mut languages: Vec<(String, PathBuf)> = fs::read_dir(&grammars)
        .unwrap_or_else(|error| panic!("{}: {error}", grammars.display()))
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| (entry.file_name().into_string().unwrap(), entry.path()))
        .collect();
    languages.sort();

    let mut table = String::from("[\n");
    for (language, dir) in &languages {
        println!("cargo:rerun-if-changed={}", dir.display());
        let file = |name: &str| format!("include_str!({:?})", dir.join(name).display().to_string());
        writeln!(
            table,
            "    ({language:?}, ExtensionFiles {{\n        \
             extension_toml: {},\n        highlights: {},\n        injections: {},\n    }}),",
            file("extension.toml"),
            file("highlights.scm"),
            file("injections.scm"),
        )
        .unwrap();
    }
    table.push(']');

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("first_party.rs");
    let source = format!(
        "/// The files of each first-party grammar's extension, by language: one\n\
         /// per directory under `extensions/grammars/`, which `build.rs` lists.\n\
         const FIRST_PARTY: &[(&str, ExtensionFiles)] = &{table};\n"
    );
    fs::write(&out, source).unwrap_or_else(|error| panic!("{}: {error}", out.display()));
}
