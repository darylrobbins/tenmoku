//! Finding a body below its header, from rows between them, and closing
//! rows found through both candidate APIs.

use std::path::Path;

use fe_buffer::Buffer;

use super::*;
use crate::language_for_path;

/// `text` parsed in the language `file` names, if it's in this build.
fn parsed(file: &str, text: &str) -> Option<Syntax> {
    let language = language_for_path(Path::new(file))?;
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    Some(syntax)
}

/// `fold_ranges` as `(first, last)` rows.
fn folds(syntax: &Syntax, rows: Range<u32>) -> Vec<(u32, u32)> {
    fold_ranges(syntax, rows)
        .into_iter()
        .map(|range| (*range.start(), *range.end()))
        .collect()
}

#[test]
fn lookup_finds_a_body_from_the_comments_before_it() {
    for (file, text) in [
        ("a.py", "def f():\n    # one\n    # two\n    x()\n"),
        ("a.lua", "function f()\n-- one\n-- two\nx()\nend"),
    ] {
        let Some(syntax) = parsed(file, text) else {
            continue;
        };
        for row in 0..3 {
            assert_eq!(fold_range_at(&syntax, row), Some(0..=3), "{file} row {row}");
        }
    }
}

#[test]
fn bodies_follow_any_number_of_comments() {
    let comments = 20;
    for (file, header, comment, body) in [
        ("a.py", "def f():\n", "    # c\n", "    x()\n"),
        ("a.lua", "function f()\n", "-- c\n", "x()\nend"),
    ] {
        let text = format!("{header}{}{body}", comment.repeat(comments));
        let Some(syntax) = parsed(file, &text) else {
            continue;
        };
        let last = comments as u32 + 1;
        let rows = text.split('\n').count() as u32;
        assert_eq!(folds(&syntax, 0..rows), [(0, last)], "{file}: every row");
        assert_eq!(folds(&syntax, 0..1), [(0, last)], "{file}: header row");
        for row in 0..last {
            assert_eq!(
                fold_range_at(&syntax, row),
                Some(0..=last),
                "{file} row {row}"
            );
        }
    }
}

#[test]
fn lookup_finds_a_body_from_blank_rows_before_it() {
    for (file, text) in [
        ("a.py", "def f():\n\n\n    x()\n    y()\n"),
        ("a.lua", "function f()\n\n\nx()\ny()\nend"),
    ] {
        let Some(syntax) = parsed(file, text) else {
            continue;
        };
        assert_eq!(folds(&syntax, 0..6), [(0, 4)], "{file}");
        for row in 0..5 {
            assert_eq!(fold_range_at(&syntax, row), Some(0..=4), "{file} row {row}");
        }
    }
}

#[test]
fn closing_rows_may_end_in_any_whitespace() {
    for trailing in [" ".repeat(13), "\t".repeat(13), " \t ".repeat(8)] {
        let text = format!("=begin\na\nb\n=end{trailing}");
        let Some(syntax) = parsed("a.rb", &text) else {
            return;
        };
        assert_eq!(folds(&syntax, 0..4), [(0, 2)], "{text:?}");
        assert_eq!(fold_range_at(&syntax, 0), Some(0..=2), "{text:?}");
        assert_eq!(fold_range_at(&syntax, 3), None, "{text:?}");
    }
}

#[test]
fn closing_rows_hold_only_the_closer() {
    for text in ["=begin\na\nb\n=end x", "=begin\na\nb\n=endx"] {
        let Some(syntax) = parsed("a.rb", text) else {
            return;
        };
        assert_eq!(fold_range_at(&syntax, 0), Some(0..=3), "{text:?}");
    }
}
