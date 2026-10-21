//! The outline scopes enclosing a row range, for sticky scroll: each outline
//! item spanning two rows or more that overlaps the rows, as
//! `header..=last`, found by running the outline query over just those rows.
//!
//! Built only with every language of `tests/outline_samples/`.

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
    feature = "html"
))]

use std::{
    fmt::Write as _,
    fs,
    ops::{Range, RangeInclusive},
    path::Path,
    time::Duration,
};

use fe_buffer::{Buffer, Text};
use fe_syntax::{Outline, Syntax, language_by_name, language_for_path, outline, outline_scopes};

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    // No deadline: a loaded machine can't make the parse give up.
    assert!(syntax.parse_until(Duration::MAX, || false));
    syntax
}

fn scopes(syntax: &Syntax, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
    outline_scopes(syntax, rows).unwrap()
}

/// The scopes `outline` gives for `rows`, found from the whole outline: its
/// items of two rows or more overlapping them, the outermost per header.
fn from_outline(outline: &Outline, text: &Text, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
    let mut scopes: Vec<RangeInclusive<u32>> = (outline.items.iter())
        .filter_map(|item| {
            let end = text.offset_to_point(item.range.end);
            let last = if end.column == 0 {
                end.row.saturating_sub(1)
            } else {
                end.row
            };
            let header = item.row;
            (header < last && header < rows.end && rows.start <= last).then_some(header..=last)
        })
        .collect();
    scopes.sort_by_key(|scope| (*scope.start(), std::cmp::Reverse(*scope.end())));
    scopes.dedup_by_key(|scope| *scope.start());
    scopes
}

const RUST: &str = "\
mod m {
    impl S {
        #[inline]
        fn f() {
            let a = 1;
            let b = 2;
        }
        fn one() {}
    }
    fn g() {
    }
}
fn after() {
    x();
}
";

#[test]
fn rust_rows_inside_a_fn_give_its_enclosing_items_outermost_first() {
    let syntax = parsed("rust", RUST);
    // m, impl S, and f from its fn row, not its #[inline].
    let enclosing = vec![0..=11, 1..=8, 3..=6];
    assert_eq!(scopes(&syntax, 4..6), enclosing);
    assert_eq!(scopes(&syntax, 4..5), enclosing);
    // A header row is inside its own item.
    assert_eq!(scopes(&syntax, 3..4), enclosing);
    // The one-row fn one() overlaps but isn't a scope; g and after don't
    // overlap.
    assert_eq!(scopes(&syntax, 4..8), enclosing);
    assert_eq!(scopes(&syntax, 7..8), vec![0..=11, 1..=8]);
    // Rows covering g and after.
    assert_eq!(scopes(&syntax, 10..13), vec![0..=11, 9..=10, 12..=14]);
    assert_eq!(scopes(&syntax, 4..4), Vec::new());
    assert_eq!(scopes(&syntax, 40..50), Vec::new());
}

#[test]
fn items_sharing_a_header_row_give_the_outermost() {
    let syntax = parsed(
        "rust",
        "mod m { impl S {\n    fn f() {\n        x();\n    }\n} }\n",
    );
    assert_eq!(scopes(&syntax, 2..3), vec![0..=4, 1..=3]);
}

#[test]
fn a_decorated_python_def_starts_at_its_def_row() {
    let text = "\
class C:
    @staticmethod
    @other
    def f():
        x = 1
        return x

@cache
def g():
    pass
";
    let syntax = parsed("python", text);
    assert_eq!(scopes(&syntax, 4..5), vec![0..=5, 3..=5]);
    assert_eq!(scopes(&syntax, 1..2), vec![0..=5]);
    assert_eq!(scopes(&syntax, 7..10), vec![8..=9]);
}

#[test]
fn markdown_sections_nest_by_level() {
    let text = "\
# A
text
## B
text
### C
text
text
## D
text
# E
text
";
    let syntax = parsed("markdown", text);
    // A's section ends at E, B's and C's at D.
    assert_eq!(scopes(&syntax, 5..6), vec![0..=8, 2..=6, 4..=6]);
    assert_eq!(scopes(&syntax, 8..9), vec![0..=8, 7..=8]);
    assert_eq!(scopes(&syntax, 10..11), vec![9..=10]);
}

/// Every row of every outline sample, and windows of rows, give what the
/// whole outline does.
#[test]
fn samples_match_the_whole_outline_row_by_row() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/outline_samples");
    let mut paths: Vec<_> = (fs::read_dir(dir).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().ends_with(".outline"))
        .collect();
    paths.sort();
    let mut checked = 0;
    for path in paths {
        let Some(language) = language_for_path(&path) else {
            continue;
        };
        let text = fs::read_to_string(&path).unwrap();
        let syntax = parsed(language.name(), &text);
        let Ok(outline) = outline(&syntax) else {
            continue;
        };
        let text = Text::new(&text);
        let rows = text.row_count();
        let mut mismatches = String::new();
        for start in 0..rows {
            for len in [1, 7] {
                let window = start..(start + len).min(rows);
                let expected = from_outline(&outline, &text, window.clone());
                let found = scopes(&syntax, window.clone());
                if found != expected {
                    writeln!(mismatches, "rows {window:?}: {found:?} != {expected:?}").unwrap();
                }
            }
        }
        assert!(mismatches.is_empty(), "{}:\n{mismatches}", path.display());
        checked += 1;
    }
    assert!(checked >= 10, "{checked} samples checked");
}

#[test]
fn deep_rows_of_a_50k_row_file_match_the_whole_outline() {
    let mut text = String::from("pub mod outer {\n");
    // 20 rows and 3 items each, under the outline's cap.
    for i in 0..2_500 {
        writeln!(text, "    pub mod m{i} {{\n        impl S{i} {{").unwrap();
        writeln!(
            text,
            "            fn f{i}(&self) {{\n                let a = {i};"
        )
        .unwrap();
        text.push_str("                if a > 0 {\n");
        text.push_str(&"                    g(a);\n".repeat(11));
        text.push_str("                }\n            }\n        }\n    }\n");
    }
    text.push_str("}\n");
    let syntax = parsed("rust", &text);
    let text = Text::new(&text);
    let rows = text.row_count();
    assert!(rows >= 50_000, "{rows} rows");
    let outline = outline(&syntax).unwrap();
    assert!(!outline.truncated);
    for window in [25_004..25_005, 25_000..25_040, 40_005..40_006, 4..5] {
        let found = scopes(&syntax, window.clone());
        assert_eq!(found, from_outline(&outline, &text, window.clone()));
        assert!(found.len() >= 3, "{window:?}: {found:?}");
        assert_eq!(found[0], 0..=rows - 2, "outer is the first");
    }
}

/// Items beyond the whole outline's cap still have their scopes, however
/// many items come before them.
#[test]
fn scopes_past_the_outline_cap_are_found() {
    let mut text = String::from("class C:\n");
    for i in 0..10_500 {
        writeln!(text, "    def m{i}(self): pass").unwrap();
    }
    text.push_str("    def last(self):\n        x = 1\n        return x\n");
    let syntax = parsed("python", &text);
    assert_eq!(
        scopes(&syntax, 10_502..10_503),
        vec![0..=10_503, 10_501..=10_503]
    );

    let mut text = String::from("# Top\n");
    for i in 0..10_500 {
        writeln!(text, "## H{i}\ntext").unwrap();
    }
    text.push_str("### Deep\ntext\n## End\ntext\n");
    let syntax = parsed("markdown", &text);
    let deep = 1 + 2 * 10_500;
    let rows = deep + 4;
    assert_eq!(
        scopes(&syntax, deep + 1..deep + 2),
        vec![0..=rows - 1, deep - 2..=deep + 1, deep..=deep + 1]
    );
    assert_eq!(
        scopes(&syntax, rows - 1..rows),
        vec![0..=rows - 1, rows - 2..=rows - 1]
    );
}
