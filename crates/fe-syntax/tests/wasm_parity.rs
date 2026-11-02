//! The first-party WebAssembly grammars (`extensions/grammars/<language>/`)
//! highlight as the built-in languages do.
//!
//! The ignored tests, one per language and named after it, load the module
//! `scripts/build-grammar-wasm.sh` builds, through the loader user grammars
//! take, and compare its highlights of the golden sample with the sample's
//! `.golden` file, which `src/tests/golden.rs` holds the built-in language to.
//! The script runs them; the gate has no wasi-sdk and the modules aren't
//! committed.
//!
//! The others need no module: they keep what the extensions hold from drifting
//! from what the built-in languages use, so a grammar bump in `Cargo.lock` or a
//! query change fails here too, on every run.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{GrammarSpec, Language, Syntax, register_language};

const SCRIPT: &str = "scripts/build-grammar-wasm.sh";

fn repository() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
}

fn extension(language: &str) -> PathBuf {
    repository().join("extensions/grammars").join(language)
}

fn read(file: &Path) -> String {
    fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The string `key = "..."` in a manifest of ours, which has one per line.
fn field(file: &Path, key: &str) -> String {
    read(file)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("{} has no {key}", file.display()))
}

/// `language`'s module in `modules`, with its extension's queries and
/// precedence, under a name of its own: a built-in language refuses its name
/// and file extensions to a registered one.
fn load(modules: &Path, language: &str) -> &'static Language {
    let dir = extension(language);
    let manifest = dir.join("extension.toml");
    let wasm = modules.join(field(&manifest, "wasm"));
    assert!(
        wasm.is_file(),
        "{} doesn't exist: build it with {SCRIPT} {language}",
        wasm.display()
    );
    register_language(&GrammarSpec {
        language: format!("{language}_wasm"),
        wasm,
        highlights: Some(dir.join(field(&manifest, "highlights"))),
        injections: Some(dir.join(field(&manifest, "injections"))),
        earlier_patterns_win: field(&manifest, "earlier_patterns_win") == "true",
        ..GrammarSpec::default()
    })
    .unwrap_or_else(|error| panic!("{language}: {error}"))
}

/// One line per highlight span, as `src/tests/golden.rs` writes them.
fn render(language: &'static Language, text: &str) -> String {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let tree = syntax.tree().expect("the sample parses");
    assert!(
        !tree.root_node().has_error(),
        "the sample parses without errors"
    );
    let spans = loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            break spans;
        }
        for name in missing {
            fe_syntax::language_by_name(name);
        }
    };
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        (point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for span in spans {
        let (row, column) = point(span.range.start);
        let (end_row, end_column) = point(span.range.end);
        let end = if end_row == row {
            end_column.to_string()
        } else {
            format!("{end_row}:{end_column}")
        };
        let name = span.highlight.name();
        let text = &text[span.range];
        out.push_str(&format!("{row}:{column}-{end} {name} {text:?}\n"));
    }
    out
}

/// `language`'s module highlights `sample` as its `.golden` file says.
fn assert_parity(language: &str, sample: &str) {
    let modules = repository().join("target/grammar-wasm");
    let loaded = load(&modules, language);
    let golden_dir = repository().join("crates/fe-syntax/tests/golden");
    let actual = render(loaded, &read(&golden_dir.join(sample)));
    let expected = read(&golden_dir.join(format!("{sample}.golden")));
    if actual != expected {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{language}'s wasm differs from {sample}.golden at line {} ({} lines, wasm {}):\n\
             - {}\n+ {}",
            line + 1,
            expected.lines().count(),
            actual.lines().count(),
            expected.lines().nth(line).unwrap_or("(end)"),
            actual.lines().nth(line).unwrap_or("(end)"),
        );
    }
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn kotlin() {
    assert_parity("kotlin", "kotlin.kt");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn csharp() {
    assert_parity("csharp", "csharp.cs");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn scala() {
    assert_parity("scala", "scala.scala");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn swift() {
    assert_parity("swift", "swift.swift");
}

#[test]
#[should_panic(expected = "scripts/build-grammar-wasm.sh kotlin")]
fn a_missing_module_fails_naming_the_script() {
    load(
        &repository().join("target/grammar-wasm/no-such-dir"),
        "kotlin",
    );
}

/// What every extension holds, and for each language anything more. No
/// module: those are built, and published as release assets.
const LANGUAGES: &[(&str, &[&str])] = &[
    // The Apache-2.0 text for highlights.scm's nvim-treesitter patterns.
    ("kotlin", &["LICENSE-APACHE"]),
    ("csharp", &[]),
    ("scala", &[]),
    ("swift", &[]),
];

#[test]
fn each_extension_holds_its_files_and_no_module() {
    for (language, more) in LANGUAGES {
        let mut files: Vec<String> = fs::read_dir(extension(language))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        let mut expected: Vec<String> = [
            "LICENSE",
            "extension.toml",
            "grammar-source.toml",
            "highlights.scm",
            "injections.scm",
        ]
        .iter()
        .chain(more.iter())
        .map(|&file| file.to_owned())
        .collect();
        expected.sort();
        assert_eq!(files, expected, "extensions/grammars/{language}");
    }
}

#[test]
fn each_pin_is_cargo_locks() {
    let lock = read(&repository().join("Cargo.lock"));
    for (language, _) in LANGUAGES {
        let source = extension(language).join("grammar-source.toml");
        let (krate, version) = (field(&source, "crate"), field(&source, "version"));
        let entry = format!("name = \"{krate}\"\nversion = \"{version}\"\n");
        assert!(
            lock.contains(&entry),
            "{language}: Cargo.lock doesn't pin {krate} {version}; update {} and rebuild \
             with {SCRIPT}",
            source.display()
        );
    }
}

#[test]
fn each_wasm_file_is_named_after_its_export() {
    for (language, _) in LANGUAGES {
        let dir = extension(language);
        let grammar = field(&dir.join("grammar-source.toml"), "grammar");
        assert_eq!(
            field(&dir.join("extension.toml"), "wasm"),
            format!("tree-sitter-{}.wasm", grammar.replace('_', "-")),
            "{language}: the loader finds tree_sitter_{grammar} by the file's name"
        );
    }
}

/// A query file with nothing but comments and blank lines.
fn has_no_patterns(query: &str) -> bool {
    query.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with(';')
    })
}

/// The extension is for the files the built-in language is, and its queries
/// are the built-in's, the parts concatenated as `language.rs` does.
/// `injections` is `None` where the built-in has no such query.
#[allow(dead_code)] // With every grammar's feature off.
fn assert_as_built_in(language: &str, highlights: &[&str], injections: Option<&str>) {
    let dir = extension(language);
    let manifest = read(&dir.join("extension.toml"));
    let extensions = manifest
        .lines()
        .find_map(|line| line.strip_prefix("extensions = "))
        .expect("extension.toml lists its extensions");
    for extension in extensions.trim_matches(['[', ']']).split(", ") {
        let path = format!("file.{}", extension.trim_matches('"'));
        assert_eq!(
            fe_syntax::language_name_for_path(Path::new(&path)),
            Some(language),
            "{path}"
        );
    }
    assert!(
        read(&dir.join("highlights.scm")) == highlights.concat(),
        "extensions/grammars/{language}/highlights.scm isn't the built-in's query"
    );
    let file = read(&dir.join("injections.scm"));
    match injections {
        Some(query) => assert!(
            file == query,
            "extensions/grammars/{language}/injections.scm isn't the built-in's query"
        ),
        None => assert!(
            has_no_patterns(&file),
            "extensions/grammars/{language}/injections.scm has patterns; the built-in has none"
        ),
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_extension_is_as_built_in() {
    assert_as_built_in(
        "kotlin",
        &[
            tree_sitter_kotlin_sg::HIGHLIGHTS_QUERY,
            include_str!("../queries/kotlin.scm"),
        ],
        None,
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_extension_is_as_built_in() {
    assert_as_built_in("csharp", &[tree_sitter_c_sharp::HIGHLIGHTS_QUERY], None);
}

#[cfg(feature = "scala")]
#[test]
fn scala_extension_is_as_built_in() {
    assert_as_built_in("scala", &[tree_sitter_scala::HIGHLIGHTS_QUERY], None);
}

#[cfg(feature = "swift")]
#[test]
fn swift_extension_is_as_built_in() {
    assert_as_built_in("swift", &[tree_sitter_swift::HIGHLIGHTS_QUERY], None);
}
