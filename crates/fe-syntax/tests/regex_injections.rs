//! Python `re.*`, Rust `Regex::new` and Go `regexp.*` strings are highlighted by
//! the hidden regex grammar: raw strings whole, escaped strings only when their
//! content has no backslash.

#![cfg(all(
    feature = "regex",
    feature = "python",
    feature = "rust",
    feature = "go"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

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

fn has(found: &[(String, &str)], text: &str, highlight: &str) -> bool {
    found.iter().any(|(t, h)| t == text && *h == highlight)
}

/// The whole literal is one `string` span: the host's, not the regex grammar's.
fn plain_string(found: &[(String, &str)], literal: &str) -> bool {
    has(found, literal, "string")
}

#[test]
fn python_re_raw_string_is_a_regex() {
    let found = spans("python", "import re\nre.compile(r\"\\d+(?P<n>x)\")\n");
    assert!(has(&found, "\\d", "escape"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "n", "label"), "{found:?}");
}

#[test]
fn python_escaped_string_with_backslash_stays_a_string() {
    let found = spans("python", "import re\nre.search(\"a\\\\d+\", s)\n");
    assert!(!has(&found, "+", "operator"), "{found:?}");
    assert!(!has(&found, "\\d", "escape"), "{found:?}");
}

#[test]
fn python_escaped_string_without_backslash_is_a_regex() {
    let found = spans("python", "import re\nre.search(\"a+b\", s)\n");
    assert!(has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn rust_regex_new_raw_string_is_a_regex() {
    let found = spans(
        "rust",
        "fn f() { let r = Regex::new(r\"\\d+(?P<n>x)\"); }\n",
    );
    assert!(has(&found, "\\d", "escape"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "n", "label"), "{found:?}");
}

#[test]
fn rust_ordinary_string_with_backslash_stays_a_string() {
    let found = spans("rust", "fn f() { let r = Regex::new(\"a\\\\d+\"); }\n");
    assert!(!has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn go_regexp_raw_string_is_a_regex() {
    let found = spans(
        "go",
        "package p\nvar r = regexp.MustCompile(`\\d+(?P<n>x)`)\n",
    );
    assert!(has(&found, "\\d", "escape"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "n", "label"), "{found:?}");
}

#[test]
fn non_regex_calls_are_not_injected() {
    let py = spans("python", "foo.compile(r\"\\d+\")\n");
    assert!(!has(&py, "+", "operator"), "{py:?}");
    let rs = spans("rust", "fn f() { Other::new(r\"\\d+\"); }\n");
    assert!(!has(&rs, "+", "operator"), "{rs:?}");
    let go = spans("go", "package p\nvar r = regexp.QuoteMeta(`\\d+`)\n");
    assert!(!has(&go, "+", "operator"), "{go:?}");
    assert!(plain_string(&go, "`\\d+`"), "{go:?}");
}
