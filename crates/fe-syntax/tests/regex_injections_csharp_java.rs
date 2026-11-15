//! C# `Regex.*` / `new Regex(..)` and Java `Pattern.compile` / `String.matches`
//! strings are highlighted by the hidden regex grammar: verbatim and raw strings
//! whole, escaped strings and Java text blocks only when their content has no
//! backslash.

#![cfg(all(feature = "regex", feature = "csharp", feature = "java"))]

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

#[test]
fn csharp_verbatim_regex_is_injected() {
    let found = spans(
        "csharp",
        "class C { bool f(string s) => Regex.IsMatch(s, @\"\\d+\"); }\n",
    );
    assert!(has(&found, "\\d", "escape"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn csharp_new_regex_is_injected() {
    let found = spans(
        "csharp",
        "class C { Regex r = new Regex(@\"\\d+\", RegexOptions.None); Regex q = new Regex(\"a+b\"); }\n",
    );
    assert!(has(&found, "\\d", "escape"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
    let raw = spans(
        "csharp",
        "class C { Regex r = new Regex(\"\"\"\\d+\"\"\"); }\n",
    );
    assert!(has(&raw, "\\d", "escape"), "{raw:?}");
}

#[test]
fn csharp_escaped_string_with_backslash_stays_a_string() {
    let found = spans(
        "csharp",
        "class C { bool f(string s) => Regex.IsMatch(s, \"a\\\\d+\"); }\n",
    );
    assert!(!has(&found, "+", "operator"), "{found:?}");
    assert!(!has(&found, "\\d", "escape"), "{found:?}");
}

#[test]
fn java_pattern_compile_without_backslash_is_injected() {
    let found = spans(
        "java",
        "class C { Pattern p = Pattern.compile(\"a+b\"); boolean m = s.matches(\"x*\"); }\n",
    );
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "*", "operator"), "{found:?}");
}

#[test]
fn java_escaped_backslash_string_stays_a_string() {
    let found = spans(
        "java",
        "class C { Pattern p = Pattern.compile(\"\\\\d+\"); }\n",
    );
    assert!(!has(&found, "+", "operator"), "{found:?}");
    assert!(!has(&found, "\\\\d", "escape"), "{found:?}");
}

#[test]
fn other_calls_are_not_injected() {
    let cs = spans(
        "csharp",
        "class C { void f() { Other.IsMatch(s, @\"\\d+\"); Regex.Escape(@\"\\d+\"); new Other(@\"\\d+\"); } }\n",
    );
    assert!(!has(&cs, "+", "operator"), "{cs:?}");
    let java = spans(
        "java",
        "class C { void f() { Pattern.quote(\"a+b\"); foo.compile(\"a+b\"); s.contains(\"a+b\"); } }\n",
    );
    assert!(!has(&java, "+", "operator"), "{java:?}");
}
