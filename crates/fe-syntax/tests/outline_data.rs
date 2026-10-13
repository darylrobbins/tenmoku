//! The outline of JSON, YAML, CSS and Dockerfile, from their queries in
//! `queries/outline/`, which are ours: none of the four grammars ships a
//! `tags.scm`. They have no definitions query, so no symbols.
//!
//! Each sample in `tests/outline_data_samples/` is outlined whole and
//! compared with `<sample>.outline` next to it, in the format of
//! `tests/outline.rs`: one line per item, indented two spaces per depth,
//! `<kind> "<name>" <range>` with the range as `row:col-row:col`, 1-based
//! rows and byte columns.
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --test outline_data`
//! rewrites them; review the diff before committing it.

#![cfg(all(
    feature = "json",
    feature = "yaml",
    feature = "css",
    feature = "dockerfile"
))]

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{
    NoOutline, Outline, OutlineKind, Syntax, language_by_name, language_for_path, language_names,
    outline, outline_supported, symbols, symbols_job, symbols_supported,
};

const LANGUAGES: [&str; 4] = ["css", "dockerfile", "json", "yaml"];

/// Read at run time: the shared cargo target can keep a compile-time path
/// to a snapshot that has since been deleted.
fn samples_dir() -> PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests")
        .join("outline_data_samples")
}

/// Every sample, with its language name and text, by language name.
/// Goldens are skipped.
fn samples() -> Vec<(PathBuf, &'static str, String)> {
    let mut samples: Vec<_> = fs::read_dir(samples_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().ends_with(".outline"))
        .map(|path| {
            let language = language_for_path(&path).unwrap().name();
            let text = fs::read_to_string(&path).unwrap();
            (path, language, text)
        })
        .collect();
    samples.sort_by_key(|&(_, language, _)| language);
    let languages: Vec<_> = samples.iter().map(|&(_, language, _)| language).collect();
    assert_eq!(languages, LANGUAGES, "one sample per language");
    samples
}

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "the {language} text parses without errors"
    );
    syntax
}

fn outlined(language: &str, text: &str) -> Outline {
    outline(&parsed(language, text)).unwrap()
}

/// An item's kind, name and its parent's name.
type Listed = (OutlineKind, String, Option<String>);

/// Each item's kind, name and parent's name, in order.
fn listed(outline: &Outline) -> Vec<Listed> {
    (outline.items.iter())
        .map(|item| {
            let parent = (item.parent).map(|ix| outline.items[ix as usize].name.clone());
            (item.kind, item.name.clone(), parent)
        })
        .collect()
}

fn item(kind: OutlineKind, name: &str, parent: Option<&str>) -> Listed {
    (kind, name.to_owned(), parent.map(str::to_owned))
}

/// Each item's name and depth, in order.
fn depths(outline: &Outline) -> Vec<(&str, u32)> {
    (outline.items.iter())
        .map(|item| (item.name.as_str(), item.depth))
        .collect()
}

fn render(outline: &Outline, text: &str) -> String {
    let buffer = Buffer::new(text);
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        format!("{}:{}", point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for item in &outline.items {
        let indent = "  ".repeat(item.depth as usize);
        let (start, end) = (point(item.range.start), point(item.range.end));
        writeln!(out, "{indent}{:?} {:?} {start}-{end}", item.kind, item.name).unwrap();
    }
    if outline.truncated {
        out.push_str("truncated\n");
    }
    out
}

#[test]
fn outlines_match_golden_files() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for (path, language, text) in samples() {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let actual = render(&outlined(language, &text), &text);
        let golden = path.with_file_name(format!("{file_name}.outline"));
        if update {
            fs::write(&golden, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&golden).unwrap_or_default();
        if expected != actual {
            failures.push(format!(
                "{file_name}.outline\n--- expected\n{expected}--- actual\n{actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "outlines differ from the golden files. If the change is intended, rerun with \
         UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn json_and_yaml_keys_nest() {
    use OutlineKind::*;
    let expected = [
        item(Module, "a", None),
        item(Field, "b", Some("a")),
        item(Field, "c", None),
        item(Field, "d", Some("c")),
    ];
    let expected_depths = [("a", 0), ("b", 1), ("c", 0), ("d", 1)];

    let json = outlined("json", "{\"a\": {\"b\": 1}, \"c\": [{\"d\": 2}]}\n");
    assert_eq!(listed(&json), expected);
    assert_eq!(depths(&json), expected_depths);

    // Empty keys, with a scalar, an object and an array of objects.
    let json = outlined("json", "{\"\": 1, \"\": {\"b\": 1}, \"\": [{\"d\": 2}]}\n");
    assert_eq!(
        listed(&json),
        [
            item(Field, "", None),
            item(Module, "", None),
            item(Field, "b", Some("")),
            item(Field, "", None),
            item(Field, "d", Some("")),
        ]
    );
    assert_eq!(
        depths(&json),
        [("", 0), ("", 0), ("b", 1), ("", 0), ("d", 1)]
    );
    // An empty name sits between the quotes.
    assert_eq!(json.items[0].name_range, 2..2);

    // The same shape in block style.
    let yaml = outlined("yaml", "a:\n  b: 1\nc:\n  - d: 2\n");
    assert_eq!(listed(&yaml), expected);
    assert_eq!(depths(&yaml), expected_depths);

    // A quoted key keeps its quotes, and a flow mapping's keys are listed.
    let yaml = outlined("yaml", "\"q k\": 1\n'p': {x: 1}\n");
    assert_eq!(
        listed(&yaml),
        [
            item(Field, "\"q k\"", None),
            item(Module, "'p'", None),
            item(Field, "x", Some("'p'")),
        ]
    );

    // Both documents of a two-document file.
    let yaml = outlined("yaml", "first: 1\n---\nsecond:\n  third: 3\n");
    assert_eq!(
        listed(&yaml),
        [
            item(Field, "first", None),
            item(Module, "second", None),
            item(Field, "third", Some("second")),
        ]
    );
}

#[test]
fn css_and_dockerfile_items() {
    use OutlineKind::*;
    let css = outlined(
        "css",
        "a,\n  b {\n  color: red;\n}\n\
         @media screen and (max-width: 600px), print {\n  .x { margin: 0; }\n}\n\
         @supports (display: grid) {\n  .g { display: grid; }\n}\n\
         @keyframes spin {\n  from { opacity: 0; }\n}\n",
    );
    assert_eq!(
        listed(&css),
        [
            item(Class, "a, b", None),
            item(Module, "screen and (max-width: 600px)", None),
            item(Class, ".x", Some("screen and (max-width: 600px)")),
            item(Module, "(display: grid)", None),
            item(Class, ".g", Some("(display: grid)")),
            item(Module, "spin", None),
        ]
    );

    // Comments before the first query, and more than one query.
    let css = outlined(
        "css",
        "@media /* c */ screen, print {\n  a { margin: 0; }\n}\n\
         @media /* c */ /* d */ (min-width: 1px) {\n  b { margin: 0; }\n}\n\
         @supports /* c */ (display: grid) {\n  c { margin: 0; }\n}\n",
    );
    assert_eq!(
        listed(&css),
        [
            item(Module, "screen", None),
            item(Class, "a", Some("screen")),
            item(Module, "(min-width: 1px)", None),
            item(Class, "b", Some("(min-width: 1px)")),
            item(Module, "(display: grid)", None),
            item(Class, "c", Some("(display: grid)")),
        ]
    );

    let text = "FROM rust:1 AS build\nRUN cargo build\n\nFROM debian\nCOPY --from=build /a /a\n";
    let docker = outlined("dockerfile", text);
    assert_eq!(
        listed(&docker),
        [item(Module, "build", None), item(Module, "debian", None)]
    );
    // Each stage is its FROM instruction alone.
    let ranges: Vec<_> = (docker.items.iter())
        .map(|item| &text[item.range.clone()])
        .collect();
    assert_eq!(ranges, ["FROM rust:1 AS build", "FROM debian"]);
}

#[test]
fn outline_but_no_symbols() {
    let texts = [
        ("json", "{\"a\": 1}\n"),
        ("yaml", "a: 1\n"),
        ("css", "a { color: red; }\n"),
        ("dockerfile", "FROM debian\n"),
    ];
    for (language, text) in texts {
        assert!(outline_supported(language), "{language}");
        assert!(!symbols_supported(language), "{language}");
        let syntax = parsed(language, text);
        assert_eq!(outline(&syntax).unwrap().items.len(), 1, "{language}");
        assert_eq!(symbols(&syntax).err(), Some(NoOutline::Unsupported));
        assert_eq!(symbols_job(&syntax).err(), Some(NoOutline::Unsupported));
    }
    // HTML and XML still have neither.
    for (language, built) in [
        ("html", cfg!(feature = "html")),
        ("xml", cfg!(feature = "xml")),
    ] {
        assert_eq!(
            language_names().any(|name| name == language),
            built,
            "{language}"
        );
        assert!(!outline_supported(language), "{language}");
        assert!(!symbols_supported(language), "{language}");
    }
}
