//! Python f-string placeholders are coloured as VS Code does: braces,
//! conversions and format specs as placeholders, the expression in its own
//! colours, `{{` and `}}` as escapes.

#![cfg(feature = "python")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text`.
fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("python").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

fn has(found: &[(String, &str)], text: &str, highlight: &str) {
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains(text) && *h == highlight),
        "missing ({text:?}, {highlight:?}) in {found:?}"
    );
}

fn none_of(found: &[(String, &str)], highlights: &[&str]) {
    for (text, highlight) in found {
        assert!(
            !highlights.iter().any(|h| highlight.contains(h)),
            "unexpected ({text:?}, {highlight:?}) in {found:?}"
        );
    }
}

#[test]
fn braces_are_placeholders() {
    let found = spans("f\"Hello {name}!\"\n");
    has(&found, "{", "string.placeholder");
    has(&found, "}", "string.placeholder");
    has(&found, "name", "variable");
    has(&found, "Hello ", "string");
    has(&found, "!", "string");
}

#[test]
fn conversion_and_format_spec_are_placeholders() {
    let found = spans("f\"{path!r}: {share:6.1%} {x:>{width}}\"\n");
    has(&found, "!r", "string.placeholder");
    has(&found, ":6.1%", "string.placeholder");
    has(&found, ":>", "string.placeholder");
    has(&found, "width", "variable");
}

#[test]
fn expression_keeps_its_colours() {
    let found = spans("f\"{a.b(1) + c}\"\n");
    assert!(
        found
            .iter()
            .any(|(t, h)| t == "b" && (h.starts_with("function") || h.starts_with("property"))),
        "{found:?}"
    );
    has(&found, "1", "number");
    has(&found, "+", "operator");
    // Every span of the expression, whitespace and parentheses included.
    let colours: Vec<_> = found
        .iter()
        .filter(|(t, h)| !t.contains('"') && *h != "string.placeholder")
        .cloned()
        .collect();
    none_of(&colours, &["string", "embedded"]);
    let nested = spans("f\"{x:{a.b(1) + c}} {y:>{w + 1}}\"\n");
    let inside: Vec<_> = nested
        .iter()
        .filter(|(t, _)| t.chars().any(|c| "abc1w+(). ".contains(c)))
        .cloned()
        .collect();
    none_of(&inside, &["embedded"]);
}

#[test]
fn escaped_braces_and_string_kinds() {
    let found = spans("f\"{{x}}\"\n");
    has(&found, "{{", "escape");
    has(&found, "}}", "escape");
    none_of(&found, &["placeholder"]);

    let found = spans("rf\"{x}\\d\"\n");
    has(&found, "{", "string.placeholder");
    has(&found, "x", "variable");

    let found = spans("f\"\"\"a {x!r}\nb {y:>3}\"\"\"\n");
    has(&found, "{", "string.placeholder");
    has(&found, "!r", "string.placeholder");
    has(&found, ":>3", "string.placeholder");

    let found = spans("f\"{d[\"k\"]}\"\n");
    has(&found, "{", "string.placeholder");
    has(&found, "\"k\"", "string");
    has(&found, "d", "variable");
}

#[test]
fn nested_format_spec_expressions_keep_their_colours() {
    let text = "f\"{x:{a.b(1) + c}}\"\n";
    let found = spans(text);
    has(&found, "a", "variable");
    has(&found, "1", "number");
    has(&found, "+", "operator");
    none_of(&found, &["embedded"]);
    // Between the nested braces, only the expression's own colours remain:
    // no placeholder, string or embedded fallback on `.`, `(`, `)` or blanks.
    let open = found.iter().position(|(t, _)| t.ends_with(":{")).unwrap();
    let close = found
        .iter()
        .rposition(|(t, _)| t.starts_with("}}"))
        .unwrap();
    let inside = &found[open + 1..close];
    assert!(!inside.is_empty(), "{found:?}");
    none_of(inside, &["placeholder", "string", "embedded"]);
}
