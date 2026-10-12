//! The outline and definition symbols of Bash, Make and SQL, from their
//! queries in `queries/outline/` and `queries/definitions/`, which are ours:
//! none of the three grammars ships a `tags.scm`.
//!
//! Each sample in `tests/outline_scripts_samples/`, named after its
//! language, is outlined and listed whole and compared with
//! `<sample>.outline` and `<sample>.symbols` next to it, in the format of
//! `tests/outline.rs`: one line per item, indented two spaces per depth,
//! `<kind> "<name>" <range>` with the range as `row:col-row:col`, 1-based
//! rows and byte columns.
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --test outline_scripts`
//! rewrites them; review the diff before committing it.

#![cfg(all(feature = "bash", feature = "make", feature = "sql"))]

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{
    Outline, OutlineKind, Syntax, language_by_name, language_for_path, language_names, outline,
    outline_supported, symbols, symbols_job, symbols_supported,
};

const LANGUAGES: [&str; 3] = ["bash", "make", "sql"];

/// Read at run time: the shared cargo target can keep a compile-time path
/// to a snapshot that has since been deleted.
fn samples_dir() -> PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests")
        .join("outline_scripts_samples")
}

/// Every sample, with its language name and text, by language name.
/// Goldens are skipped.
fn samples() -> Vec<(PathBuf, &'static str, String)> {
    let mut samples: Vec<_> = fs::read_dir(samples_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.to_string_lossy();
            !name.ends_with(".outline") && !name.ends_with(".symbols")
        })
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

/// The outline and the symbols of `language`'s sample.
fn sample(language: &str) -> (Outline, Outline) {
    let (_, _, text) = (samples().into_iter())
        .find(|&(_, name, _)| name == language)
        .unwrap();
    let syntax = parsed(language, &text);
    (outline(&syntax).unwrap(), symbols(&syntax).unwrap())
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

/// The items of `kind`, with their parents.
fn of_kind(outline: &Outline, kind: OutlineKind) -> Vec<Listed> {
    let mut items = listed(outline);
    items.retain(|(item_kind, _, _)| *item_kind == kind);
    items
}

fn item(kind: OutlineKind, name: &str, parent: Option<&str>) -> Listed {
    (kind, name.to_owned(), parent.map(str::to_owned))
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
fn outlines_and_symbols_match_golden_files() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for (path, language, text) in samples() {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            path.file_stem().unwrap().to_string_lossy(),
            language,
            "{file_name} is named after its language"
        );
        let syntax = parsed(language, &text);
        let outline = outline(&syntax).unwrap();
        let symbols = symbols(&syntax).unwrap();
        let background = symbols_job(&syntax).unwrap();
        let background = std::thread::spawn(move || background.run());
        assert_eq!(background.join().unwrap(), symbols, "{file_name}");
        // The symbols are the outline's items and more.
        for item in &outline.items {
            assert!(
                (symbols.items.iter()).any(|symbol| symbol.kind == item.kind
                    && symbol.name == item.name
                    && symbol.range == item.range),
                "{file_name}: the symbols keep {:?} {:?}",
                item.kind,
                item.name
            );
        }
        for (suffix, listed) in [(".outline", &outline), (".symbols", &symbols)] {
            let actual = render(listed, &text);
            let golden = path.with_file_name(format!("{file_name}{suffix}"));
            if update {
                fs::write(&golden, &actual).unwrap();
                continue;
            }
            let expected = fs::read_to_string(&golden).unwrap_or_default();
            if expected != actual {
                failures.push(format!(
                    "{file_name}{suffix}\n--- expected\n{expected}--- actual\n{actual}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "outlines or symbols differ from the golden files. If the change is intended, rerun \
         with UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn bash_lists_both_function_forms_and_top_level_variables() {
    use OutlineKind::*;
    let (outline, symbols) = sample("bash");
    // f() {}, function g {} and function g() {}; one in another's body
    // nests under it.
    assert_eq!(
        listed(&outline),
        [
            item(Function, "local_only", None),
            item(Function, "log", None),
            item(Function, "build", None),
            item(Function, "inner", Some("build")),
            item(Function, "upload", None),
        ]
    );
    // Each function's range is its whole definition, from its name or
    // `function` to the closing brace.
    let text = fs::read_to_string(samples_dir().join("bash.sh")).unwrap();
    for (name, starts) in [("log", "log() {"), ("build", "function build {")] {
        let item = (outline.items.iter())
            .find(|item| item.name == name)
            .unwrap();
        let defined = &text[item.range.clone()];
        assert!(defined.starts_with(starts), "{name}: {defined:?}");
        assert!(defined.ends_with('}'), "{name}: {defined:?}");
    }
    // Plain, exported, readonly and declared assignments at the top, two
    // on a line, and an array element's as the array's; not a local, an
    // assignment in a function, or a loop variable.
    assert_eq!(
        of_kind(&symbols, Variable),
        [
            item(Variable, "TARGET", None),
            item(Variable, "RETRIES", None),
            item(Variable, "LOG_DIR", None),
            item(Variable, "HOST", None),
            item(Variable, "PORT", None),
            item(Variable, "hosts", None),
            item(Variable, "extra", None),
        ]
    );
}

#[test]
fn make_lists_targets_and_defines_but_not_special_or_pattern_targets() {
    use OutlineKind::*;
    let (outline, symbols) = sample("make");
    // Targets by their text: archive members, lib.a(a.o), and ones with
    // expansions, $(OUT)/stamp.
    assert_eq!(
        listed(&outline),
        [
            item(Function, "all", None),
            item(Function, "tool", None),
            item(Function, "install", None),
            item(Function, "uninstall", None),
            item(Function, "libutil.a(util.o)", None),
            item(Function, "$(OUT)/stamp", None),
            item(Function, "help", None),
            item(Function, "lib.a(help.o)", None),
            item(Macro, "compile-rule", None),
            item(Function, "clean", None),
        ]
    );
    for item in &outline.items {
        assert!(!item.name.starts_with('.'), "{}", item.name);
        assert!(!item.name.contains('%'), "{}", item.name);
    }
    // A target alone in its rule is the whole rule, recipe and all.
    let text = fs::read_to_string(samples_dir().join("make.mk")).unwrap();
    let tool = (outline.items.iter())
        .find(|item| item.name == "tool")
        .unwrap();
    assert!(text[tool.range.clone()].contains("-o $@"));
    // Assignments with each operator, with override and in a conditional;
    // not a target's own, tool: CFLAGS += -g.
    assert_eq!(
        of_kind(&symbols, Variable),
        [
            item(Variable, "CC", None),
            item(Variable, "CFLAGS", None),
            item(Variable, "OBJS", None),
            item(Variable, "LDFLAGS", None),
            item(Variable, "EXE", None),
            item(Variable, "EXE", None),
        ]
    );
}

#[test]
fn sql_lists_tables_views_functions_and_types_with_columns() {
    use OutlineKind::*;
    let (outline, symbols) = sample("sql");
    // public.orders and reporting.daily_totals by their last names.
    assert_eq!(
        listed(&outline),
        [
            item(Table, "users", None),
            item(Table, "orders", None),
            item(Table, "active_users", None),
            item(Table, "daily_totals", None),
            item(Type, "mood", None),
            item(Function, "order_count", None),
            // Between comments, and not the SETOF users it returns.
            item(Function, "find_user", None),
            // Quoted, with its quotes.
            item(Table, "\"audit log\"", None),
        ]
    );
    // Each table's columns, nested under it; a view's aren't listed.
    assert_eq!(
        of_kind(&symbols, Field),
        [
            item(Field, "id", Some("users")),
            item(Field, "email", Some("users")),
            item(Field, "created_at", Some("users")),
            item(Field, "id", Some("orders")),
            item(Field, "user_id", Some("orders")),
            item(Field, "total", Some("orders")),
            item(Field, "\"when\"", Some("\"audit log\"")),
            item(Field, "`who`", Some("\"audit log\"")),
        ]
    );
}

#[test]
fn supported_flags() {
    for language in LANGUAGES {
        assert!(outline_supported(language), "{language}");
        assert!(symbols_supported(language), "{language}");
    }
    // The other languages with neither, as before.
    for language in ["json", "yaml", "html", "css", "xml", "dockerfile"] {
        assert!(language_names().any(|name| name == language), "{language}");
        assert!(!outline_supported(language), "{language}");
        assert!(!symbols_supported(language), "{language}");
    }
}
