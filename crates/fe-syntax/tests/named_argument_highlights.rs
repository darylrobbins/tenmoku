//! A name that labels a value is a `property`: a call's keyword argument in
//! Python and a named argument in Kotlin. A definition's parameter and a
//! positional argument keep their highlights.

#![cfg(any(feature = "python", feature = "kotlin"))]
#![allow(dead_code)]

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

/// The highlight of the span that is exactly `word`, its first occurrence.
fn at(language: &str, text: &str, word: &str) -> Option<&'static str> {
    let start = text.find(word).unwrap();
    spans(language, text)
        .into_iter()
        .rev()
        .find(|(s, t, _)| *s == start && t == word)
        .map(|(_, _, h)| h)
}

#[cfg(feature = "python")]
#[test]
fn python_keyword_arguments_are_properties() {
    assert_eq!(at("python", "f(a, key=1, **kw)\n", "key"), Some("property"));
    // A definition's parameter is not a call-site label; it stays as it was.
    assert_eq!(
        at("python", "def f(key=1): pass\n", "key"),
        Some("variable")
    );
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_named_arguments_are_properties() {
    assert_eq!(
        at("kotlin", "fun m() { f(1, name = \"x\") }\n", "name"),
        Some("property")
    );
    assert_ne!(at("kotlin", "fun m() { f(x) }\n", "x"), Some("property"));
}
