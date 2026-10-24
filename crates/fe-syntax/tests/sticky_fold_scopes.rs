//! The fold candidates enclosing a row range, for sticky scroll when a
//! language has a tree but no outline: each candidate the fold gutter has
//! that overlaps the rows, as `header..=last`, outermost first, found by
//! walking only the tree over the rows.

#![cfg(all(feature = "json", feature = "python"))]

use std::{
    fmt::Write as _,
    ops::{Range, RangeInclusive},
    time::Duration,
};

use fe_buffer::Buffer;
use fe_syntax::{Syntax, fold_ranges, fold_scopes, language_by_name};

/// `text` parsed whole as `language`.
fn parsed(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    // No deadline: a loaded machine can't make the parse give up.
    assert!(syntax.parse_until(Duration::MAX, || false));
    syntax
}

/// The scopes for `rows` found from every fold range of the file: those
/// overlapping them, the outermost per header.
fn from_all_folds(syntax: &Syntax, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
    let mut scopes: Vec<_> = fold_ranges(syntax, 0..u32::MAX)
        .into_iter()
        .filter(|fold| *fold.start() < rows.end && rows.start <= *fold.end())
        .collect();
    scopes.sort_by_key(|scope| (*scope.start(), std::cmp::Reverse(*scope.end())));
    scopes.dedup_by_key(|scope| *scope.start());
    scopes
}

const JSON: &str = r#"{
  "name": "x",
  "outer": {
    "inner": {
      "a": 1,
      "b": [
        2,
        3
      ]
    },
    "c": 4
  },
  "after": { "d": 5,
    "e": 6 }
}
"#;

#[test]
fn nested_json_objects_give_their_header_and_last_rows_outermost_first() {
    let syntax = parsed("json", JSON);
    // The root object, "outer" and "inner", each ending on the row before
    // its closing brace, as the fold gutter folds them.
    assert_eq!(fold_scopes(&syntax, 4..5), [0..=13, 2..=10, 3..=8]);
    // A header row is inside its own scope; b's array is one too.
    assert_eq!(fold_scopes(&syntax, 3..4), [0..=13, 2..=10, 3..=8]);
    assert_eq!(fold_scopes(&syntax, 6..7), [0..=13, 2..=10, 3..=8, 5..=7]);
    // inner's `}` row is past it; "after"'s object ends on its last row.
    assert_eq!(fold_scopes(&syntax, 9..10), [0..=13, 2..=10]);
    assert_eq!(fold_scopes(&syntax, 12..14), [0..=13, 12..=13]);
    // Rows covering several, and none.
    assert_eq!(fold_scopes(&syntax, 1..8), [0..=13, 2..=10, 3..=8, 5..=7]);
    assert_eq!(fold_scopes(&syntax, 4..4), []);
    assert_eq!(fold_scopes(&syntax, 40..50), []);
}

#[test]
fn every_window_of_a_sample_matches_the_whole_files_folds() {
    let python = "\
class C:
    def f(self):
        x = [
            1,
            2,
        ]
        return x

    def g(self):
        pass
d = {
    'a': 1,
}
";
    for (language, text) in [("json", JSON), ("python", python)] {
        let syntax = parsed(language, text);
        let rows = text.lines().count() as u32;
        for start in 0..rows {
            for len in [1, 3, 7] {
                let window = start..(start + len).min(rows);
                assert_eq!(
                    fold_scopes(&syntax, window.clone()),
                    from_all_folds(&syntax, window.clone()),
                    "{language} {window:?}"
                );
            }
        }
    }
}

#[test]
fn deep_rows_of_a_large_json_file_match_the_whole_files_folds() {
    let mut text = String::from("{\n");
    for i in 0..20_000 {
        writeln!(
            text,
            "  \"k{i}\": {{\n    \"a\": [\n      {i}\n    ]\n  }},"
        )
        .unwrap();
    }
    text.push_str("  \"end\": 0\n}\n");
    let syntax = parsed("json", &text);
    for window in [50_003..50_004, 50_000..50_012, 99_999..100_002, 1..2] {
        let found = fold_scopes(&syntax, window.clone());
        assert_eq!(found, from_all_folds(&syntax, window.clone()), "{window:?}");
        assert_eq!(found[0], 0..=100_001, "{window:?}: the root object first");
    }
}
