//! Regex literals in JavaScript and TypeScript are highlighted by the hidden
//! regex grammar, injected into their patterns.

#![cfg(all(feature = "regex", feature = "javascript"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, language_names};

fn spans(language: &str, text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name(language).unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return spans
                .into_iter()
                .map(|span| (text[span.range].to_string(), span.highlight.name()))
                .collect();
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

fn pairs(expected: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    expected
        .iter()
        .map(|&(text, highlight)| (text.to_string(), highlight))
        .collect()
}

const LITERAL: &str = r"const r = /^(?<y>\d{4})-[a-z]+\b/gi;";

/// The spans of [`LITERAL`]'s pattern, between its slashes.
const PATTERN: &[(&str, &str)] = &[
    ("^", "keyword"),
    ("(?<", "punctuation.bracket"),
    ("y", "label"),
    (">", "punctuation.bracket"),
    (r"\d", "escape"),
    ("{4}", "operator"),
    (")", "punctuation.bracket"),
    ("-", "string.special"),
    ("[", "punctuation.bracket"),
    ("a", "string.special"),
    ("-", "operator"),
    ("z", "string.special"),
    ("]", "punctuation.bracket"),
    ("+", "operator"),
    (r"\b", "keyword"),
];

/// `found` holds `PATTERN`'s spans in a row, after an opening `/`.
fn assert_pattern(found: &[(String, &'static str)]) {
    let pattern = pairs(PATTERN);
    let at = found
        .windows(pattern.len() + 1)
        .position(|window| window[0].0 == "/" && window[1..] == pattern[..]);
    assert!(at.is_some(), "{found:?}");
}

#[test]
fn regex_grammar_is_hidden() {
    assert!(!language_names().any(|name| name == "regex"));
    assert!(language_by_name("regex").is_some());
    let found = spans("regex", "a+");
    assert_eq!(found, pairs(&[("a", "string.special"), ("+", "operator")]));
}

#[test]
fn regex_literal_in_js_is_highlighted_by_the_regex_grammar() {
    let found = spans("javascript", LITERAL);
    assert_pattern(&found);
    // The flags are the host's, outside the injected pattern.
    assert!(
        found.contains(&("gi".to_string(), "string.special")),
        "{found:?}"
    );

    // The rest of the syntax, each kind once. Adjacent runs of one highlight
    // merge, so `-` keeps the backreferences apart.
    let found = spans("javascript", r"x = /a.(?:b|c)*?\k<n>-\1$/;");
    let expected = pairs(&[
        ("a", "string.special"),
        (".", "operator"),
        ("(?:", "punctuation.bracket"),
        ("b", "string.special"),
        ("|", "operator"),
        ("c", "string.special"),
        (")", "punctuation.bracket"),
        // A quantifier and its lazy `?`: one run of the same highlight.
        ("*?", "operator"),
        (r"\k<", "variable.builtin"),
        ("n", "label"),
        (">", "variable.builtin"),
        ("-", "string.special"),
        (r"\1", "variable.builtin"),
        ("$", "keyword"),
    ]);
    assert!(
        found
            .windows(expected.len())
            .any(|window| window == expected),
        "{found:?}"
    );
}

#[cfg(feature = "typescript")]
#[test]
fn regex_literal_in_ts_and_tsx_too() {
    for language in ["typescript", "tsx"] {
        assert_pattern(&spans(language, LITERAL));
    }
}
