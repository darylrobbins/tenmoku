//! A key written in code is a `property`; data files keep `string.special.key`.

#![cfg(all(
    feature = "javascript",
    feature = "typescript",
    feature = "rust",
    feature = "go",
    feature = "c"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(start, text, highlight)` for every span of `text` in the language `name`.
fn spans(name: &str, text: &str) -> Vec<(usize, String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name(name).unwrap(), Buffer::new(text).current());
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| {
            let start = span.range.start;
            (start, text[span.range].to_string(), span.highlight.name())
        })
        .collect()
}

#[track_caller]
fn assert_key(lang: &str, text: &str, word: &str, highlight: &str) {
    let found = spans(lang, text);
    let start = text.find(word).unwrap();
    let got = found
        .iter()
        .find(|(s, t, _)| *s == start && t == word)
        .map(|(_, _, h)| *h);
    assert_eq!(got, Some(highlight), "{lang} {word}: {found:?}");
}

#[test]
fn code_literal_keys_are_properties() {
    assert_key("javascript", "const o = {a: 1};", "a", "property");
    assert_key("typescript", "const o = {a: 1};", "a", "property");
    assert_key(
        "rust",
        "fn f() { let p = Point { x: 1 }; }",
        "x",
        "property",
    );
    assert_key(
        "rust",
        "fn f(p: P) { let P { y: z, .. } = p; }",
        "y",
        "property",
    );
    assert_key("go", "package p\nvar a = Point{X: 1}\n", "X", "property");
    assert_key("c", "struct p q = { .x = 1 };", "x", "property");
}

#[test]
fn data_file_keys_stay_string_special_key() {
    assert_key("toml", "a = 1\n", "a", "string.special.key");
    assert_key("json", "{\"a\": 1}", "\"a\"", "string.special.key");
}
