//! Definition symbols: the outline plus fields, enum variants and variables,
//! from `queries/definitions/` after the outline queries.
//!
//! Each sample in `tests/symbols_samples/`, named after its language, is
//! listed whole and compared with `<sample>.symbols` next to it, in the
//! format of `tests/outline.rs`: one line per item, indented two spaces per
//! depth, `<kind> "<name>" <range>` with the range as `row:col-row:col`,
//! 1-based rows and byte columns.
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --test symbols` rewrites
//! them; review the diff before committing it.
//!
//! Built only with every outlined language, as `tests/outline.rs` is.

#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "javascript",
    feature = "typescript",
    feature = "go",
    feature = "java",
    feature = "c",
    feature = "markdown",
    feature = "toml",
    feature = "json"
))]

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{
    NoOutline, Outline, OutlineItem, OutlineKind, Syntax, language_by_name, language_for_path,
    language_names, outline, outline_supported, symbols, symbols_job, symbols_supported,
};

const SUFFIX: &str = ".symbols";

/// Read at run time: the shared cargo target can keep a compile-time path
/// to a snapshot that has since been deleted.
fn tests_dir() -> PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tests")
}

/// Every sample in `dir` whose language is in this build, with its language
/// name and text. Goldens, named `<sample>.<suffix>`, are skipped.
fn samples(dir: &str) -> Vec<(PathBuf, &'static str, String)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(tests_dir().join(dir))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.to_string_lossy();
            !name.ends_with(SUFFIX) && !name.ends_with(".outline")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let language = language_for_path(&path)?;
            let text = fs::read_to_string(&path).unwrap();
            Some((path, language.name(), text))
        })
        .collect()
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

fn symbols_of(language: &str, text: &str) -> Outline {
    symbols(&parsed(language, text)).unwrap()
}

/// The kind and name of an item's parent.
fn parent(symbols: &Outline, item: &OutlineItem) -> Option<(OutlineKind, String)> {
    let parent = &symbols.items[item.parent? as usize];
    Some((parent.kind, parent.name.clone()))
}

/// An item's kind, name and parent's kind and name.
type Listed<'a> = (OutlineKind, &'a str, Option<(OutlineKind, String)>);

/// Each item's kind, name and parent, in order.
fn listed(symbols: &Outline) -> Vec<Listed<'_>> {
    (symbols.items.iter())
        .map(|item| (item.kind, item.name.as_str(), parent(symbols, item)))
        .collect()
}

/// The items of the given kinds, with their parents.
fn of_kinds<'a>(symbols: &'a Outline, kinds: &[OutlineKind]) -> Vec<Listed<'a>> {
    let mut items = listed(symbols);
    items.retain(|(kind, _, _)| kinds.contains(kind));
    items
}

fn under(kind: OutlineKind, name: &str) -> Option<(OutlineKind, String)> {
    Some((kind, name.to_owned()))
}

fn render(symbols: &Outline, text: &str) -> String {
    let buffer = Buffer::new(text);
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        format!("{}:{}", point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for item in &symbols.items {
        let indent = "  ".repeat(item.depth as usize);
        let (start, end) = (point(item.range.start), point(item.range.end));
        writeln!(out, "{indent}{:?} {:?} {start}-{end}", item.kind, item.name).unwrap();
    }
    if symbols.truncated {
        out.push_str("truncated\n");
    }
    out
}

#[test]
fn symbols_match_golden_files() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut covered = Vec::new();
    let mut failures = Vec::new();
    for (path, language, text) in samples("symbols_samples") {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            path.file_stem().unwrap().to_string_lossy(),
            language,
            "{file_name} is named after its language"
        );
        covered.push(language);
        let syntax = parsed(language, &text);
        let symbols = symbols(&syntax).unwrap();
        assert_eq!(symbols.version, syntax.version());
        let background = symbols_job(&syntax).unwrap();
        let background = std::thread::spawn(move || background.run());
        assert_eq!(background.join().unwrap(), symbols, "{file_name}");
        let actual = render(&symbols, &text);
        let golden = path.with_file_name(format!("{file_name}{SUFFIX}"));
        if update {
            fs::write(&golden, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&golden).unwrap_or_default();
        if expected != actual {
            failures.push(format!(
                "{file_name}{SUFFIX}\n--- expected\n{expected}--- actual\n{actual}"
            ));
        }
    }
    let mut supported: Vec<_> = language_names()
        .filter(|name| symbols_supported(name))
        .collect();
    supported.sort_unstable();
    // The languages T113 added have their samples, and goldens, in
    // more_languages_samples/, which tests/more_languages.rs checks.
    covered.extend(
        fs::read_dir(tests_dir().join("more_languages_samples"))
            .unwrap()
            .filter_map(|entry| language_for_path(&entry.unwrap().path()))
            .map(|language| language.name()),
    );
    covered.sort_unstable();
    assert_eq!(
        covered, supported,
        "every language with symbols has a sample"
    );
    assert!(
        failures.is_empty(),
        "symbols differ from the golden files. If the change is intended, rerun with \
         UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn symbols_extend_the_outline() {
    let mut checked = 0;
    let samples = samples("outline_samples")
        .into_iter()
        .chain(samples("symbols_samples"));
    for (path, language, text) in samples {
        if !symbols_supported(language) {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy();
        let syntax = parsed(language, &text);
        let outline = outline(&syntax).unwrap();
        let symbols = symbols(&syntax).unwrap();
        for item in &outline.items {
            let parent_name = item.parent.map(|ix| &outline.items[ix as usize].name);
            let found = symbols.items.iter().find(|symbol| {
                symbol.range == item.range
                    && symbol.kind == item.kind
                    && symbol.name == item.name
                    && symbol.name_range == item.name_range
                    && symbol.parent.map(|ix| &symbols.items[ix as usize].name) == parent_name
            });
            assert!(found.is_some(), "{file}: {:?} {:?}", item.kind, item.name);
            checked += 1;
        }
        assert!(
            symbols.items.len() > outline.items.len(),
            "{file} has symbols the outline leaves out"
        );
    }
    assert!(checked > 0);
}

#[test]
fn rust_fields_and_variants_nest() {
    use OutlineKind::*;
    let text = "\
struct S { a: u8, b: String }
enum E { A, B(u8), C { x: i32 } }
struct T(u8, String);
fn f() { let local = 1; }
";
    let symbols = symbols_of("rust", text);
    assert_eq!(
        listed(&symbols),
        [
            (Struct, "S", None),
            (Field, "a", under(Struct, "S")),
            (Field, "b", under(Struct, "S")),
            (Enum, "E", None),
            (Variant, "A", under(Enum, "E")),
            (Variant, "B", under(Enum, "E")),
            (Variant, "C", under(Enum, "E")),
            (Struct, "T", None),
            (Function, "f", None),
        ]
    );
}

#[test]
fn variables_are_module_level_only() {
    use OutlineKind::*;
    let python = symbols_of(
        "python",
        "x = 1\nMAX = 3\n\nclass K:\n    y = 2\n\ndef g():\n    z = 3\n",
    );
    assert_eq!(
        listed(&python),
        [
            (Variable, "x", None),
            (Constant, "MAX", None),
            (Class, "K", None),
            (Field, "y", under(Class, "K")),
            (Function, "g", None),
        ]
    );

    let javascript = symbols_of(
        "javascript",
        "const a = 1;\nexport let b = 2;\nconst f = () => 1;\nfunction h() {\n  const c = 3;\n}\n",
    );
    assert_eq!(
        listed(&javascript),
        [
            (Variable, "a", None),
            (Variable, "b", None),
            (Function, "f", None),
            (Function, "h", None),
        ]
    );
}

#[test]
fn members_of_ts_go_java_and_c() {
    use OutlineKind::*;
    let members = [Field, Variant, Variable];
    let typescript = symbols_of(
        "typescript",
        "interface I {\n  p: number;\n}\nenum Color {\n  Red,\n  Green = 2,\n}\n",
    );
    assert_eq!(
        of_kinds(&typescript, &members),
        [
            (Field, "p", under(Interface, "I")),
            (Variant, "Red", under(Enum, "Color")),
            (Variant, "Green", under(Enum, "Color")),
        ]
    );

    let go = symbols_of(
        "go",
        "package p\n\ntype S struct {\n\ta, b int\n}\n\nvar v = 1\n\nfunc f() {\n\tvar w = 2\n\t_ = w\n}\n",
    );
    assert_eq!(
        of_kinds(&go, &members),
        [
            (Field, "a", under(Struct, "S")),
            (Field, "b", under(Struct, "S")),
            (Variable, "v", None),
        ]
    );

    let java = symbols_of(
        "java",
        "class A {\n  int count;\n  void m() { int local = 0; }\n}\nenum E { ONE, TWO }\n",
    );
    assert_eq!(
        of_kinds(&java, &members),
        [
            (Field, "count", under(Class, "A")),
            (Variant, "ONE", under(Enum, "E")),
            (Variant, "TWO", under(Enum, "E")),
        ]
    );

    let c = symbols_of(
        "c",
        "struct s { int x; };\nenum e { ONE };\nint g;\nint f(void);\nint *h(void);\nint (*fp)(void);\n",
    );
    assert_eq!(
        of_kinds(&c, &members),
        [
            (Field, "x", under(Struct, "s")),
            (Variant, "ONE", under(Enum, "e")),
            (Variable, "g", None),
            (Variable, "fp", None),
        ]
    );
}

#[test]
fn unsupported_languages_have_no_symbols() {
    for (language, text) in [
        ("markdown", "# A\n"),
        ("toml", "[a]\nb = 1\n"),
        ("json", "{\"a\": 1}\n"),
    ] {
        let syntax = parsed(language, text);
        assert!(!symbols_supported(language), "{language}");
        assert_eq!(symbols(&syntax).err(), Some(NoOutline::Unsupported));
        assert_eq!(symbols_job(&syntax).err(), Some(NoOutline::Unsupported));
    }
    assert!(outline_supported("markdown") && outline_supported("toml"));
    let code = [
        "rust",
        "python",
        "javascript",
        "typescript",
        "tsx",
        "go",
        "java",
        "c",
        // Added by T113.
        "cpp",
        "csharp",
        "ruby",
        "php",
        "swift",
        "kotlin",
        "scala",
        "lua",
    ];
    for language in code {
        assert!(symbols_supported(language), "{language}");
    }
    let mut supported: Vec<_> = language_names()
        .filter(|name| symbols_supported(name))
        .collect();
    supported.sort_unstable();
    let mut code = code.to_vec();
    code.sort_unstable();
    assert_eq!(supported, code);

    let rust = language_by_name("rust").unwrap();
    let line = "struct S { a: u8 }\n";
    let len = rust.parse_limit() + 1;
    let mut text = line.repeat(len / line.len());
    text.push_str(&" ".repeat(len - text.len()));
    let huge = Buffer::new(&text);
    let syntax = Syntax::new(rust, huge.current());
    assert_eq!(symbols(&syntax).err(), Some(NoOutline::TooLarge));
}
