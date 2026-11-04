//! Ignore files colour names, anchors, directory slashes, wildcards, classes,
//! escapes and negated lines, not just comments, `!` and `*`.

#![cfg(feature = "gitignore")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text` in the gitignore language.
fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("gitignore").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

/// The highlight of the span that is exactly `text` at byte `at`.
fn at(text: &str, at: usize, len: usize) -> Option<&'static str> {
    let mut syntax = Syntax::new(
        language_by_name("gitignore").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .rfind(|s| s.range.start <= at && at + len <= s.range.end)
        .map(|s| s.highlight.name())
}

#[test]
fn directory_and_anchor_slashes_are_distinct() {
    let inner = at("a/b\n", 1, 1);
    assert!(inner.is_some());
    let trailing = at("build/\n", 5, 1);
    let leading = at("/dist\n", 0, 1);
    assert!(trailing.is_some() && leading.is_some());
    assert_ne!(trailing, inner);
    assert_ne!(leading, inner);
    for (text, offset) in [("build/\n", 0), ("/dist\n", 1), ("a/b\n", 0)] {
        assert!(at(text, offset, 1).is_some(), "{text:?} name is plain");
    }
}

#[test]
fn negated_line_is_styled_as_a_whole() {
    let found = spans("!keep.txt\n");
    let plain = spans("keep.txt\n");
    let bang = at("!keep.txt\n", 0, 1);
    let name = at("!keep.txt\n", 1, 1);
    assert!(bang.is_some(), "{found:?}");
    assert_eq!(bang, name, "{found:?}");
    assert_ne!(name, at("keep.txt\n", 0, 1), "{plain:?}");
}

#[test]
fn wildcards_classes_and_escapes_are_coloured() {
    let text = "**/a*?[a-z]\\#x\n";
    let found = spans(text);
    for (piece, offset, len) in [
        ("**", 0, 2),
        ("*", 4, 1),
        ("?", 5, 1),
        ("[", 6, 1),
        ("-", 8, 1),
        ("\\#", 11, 2),
    ] {
        assert!(at(text, offset, len).is_some(), "{piece:?} in {found:?}");
    }
    assert_ne!(at(text, 0, 2), at(text, 4, 1), "{found:?}");
}

#[test]
fn comments_and_dockerignore_names_still_work() {
    assert!(spans("# note\n").contains(&("# note".to_string(), "comment")));
    assert_eq!(
        fe_syntax::language_for_path(std::path::Path::new(".dockerignore")).map(|l| l.name()),
        Some("gitignore")
    );
}
