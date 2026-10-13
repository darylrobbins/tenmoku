#![cfg(all(feature = "go", feature = "bash", feature = "python"))]

use std::ops::Range;

use fe_buffer::Buffer;
use fe_syntax::{HighlightSpan, Syntax, language_by_name};

/// Highlights of `rows` in a fresh parse of `text`, cut to `rows`.
fn highlights(
    language: &str,
    text: &str,
    query: Range<usize>,
    cut: &Range<usize>,
) -> Vec<HighlightSpan> {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(query)
        .into_iter()
        .filter(|span| span.range.start < cut.end && span.range.end > cut.start)
        .map(|mut span| {
            span.range = span.range.start.max(cut.start)..span.range.end.min(cut.end);
            span
        })
        .collect()
}

fn row_start(text: &str, row: usize) -> usize {
    text.split_inclusive('\n').take(row).map(str::len).sum()
}

fn check(language: &str, text: &str) {
    for row in [1, 3, 1001] {
        let range = row_start(text, row)..row_start(text, row + 20);
        let from_blank = highlights(language, text, range.clone(), &range);
        let earlier = row_start(text, row - 1)..range.end;
        let from_earlier = highlights(language, text, earlier, &range);
        assert!(!from_earlier.is_empty(), "{language} row {row}");
        assert_eq!(from_blank, from_earlier, "{language} row {row}");
    }
}

#[test]
fn go_window_starting_on_a_blank_row_keeps_its_highlights() {
    let text = format!(
        "package main\n\n{}",
        "func f() int { return 1 }\n\n".repeat(1100)
    );
    check("go", &text);
}

#[test]
fn bash_window_starting_on_a_blank_row_keeps_its_highlights() {
    check("bash", &"echo \"$HOME\" # c\n\n".repeat(1100));
}

#[test]
fn python_window_starting_on_a_blank_row_keeps_its_highlights() {
    check("python", &"def f(x): return None\n\n".repeat(1100));
}
