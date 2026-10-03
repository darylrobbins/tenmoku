//! Checks on highlight queries we override or rely on for key colours.

use fe_buffer::Buffer;

use crate::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text` in `language`.
fn highlighted(language: &str, text: &str) -> Vec<(String, &'static str)> {
    let buffer = Buffer::new(text);
    let language = language_by_name(language).unwrap();
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

fn has(spans: &[(String, &'static str)], text: &str, highlight: &str) -> bool {
    spans.iter().any(|(t, h)| t == text && *h == highlight)
}

#[test]
fn toml_keys_are_properties() {
    let text = "[a.b]\nname = 1\ndotted.key = \"v\"\n[[bin]]\n";
    let spans = highlighted("toml", text);
    for (text, highlight) in [
        ("name", "property"),
        ("dotted", "property"),
        ("key", "property"),
        ("a", "type"),
        ("b", "type"),
        ("bin", "type"),
        ("\"v\"", "string"),
    ] {
        assert!(has(&spans, text, highlight), "{text}: {spans:?}");
    }
    for (span, _) in &spans {
        assert!(!span.trim().is_empty(), "whitespace-only span in {spans:?}");
    }
}

#[test]
fn toml_deep_and_quoted_keys() {
    let text = "[a.b.c.d]\n[[e.\"f\".'g']]\nx.y.z = 1\n\"q\".'r'.s = \"v\"\n\"k\" = 3\n";
    let spans = highlighted("toml", text);
    for (text, highlight) in [
        ("a", "type"),
        ("b", "type"),
        ("c", "type"),
        ("d", "type"),
        ("e", "type"),
        ("\"f\"", "type"),
        ("'g'", "type"),
        ("x", "property"),
        ("y", "property"),
        ("z", "property"),
        ("\"q\"", "property"),
        ("'r'", "property"),
        ("s", "property"),
        ("\"k\"", "property"),
        ("\"v\"", "string"),
    ] {
        assert!(has(&spans, text, highlight), "{text}: {spans:?}");
    }
}

/// A key of `n` components named `k0`, `k1`, ….
fn key(n: usize) -> String {
    (0..n)
        .map(|i| format!("k{i}"))
        .collect::<Vec<_>>()
        .join(".")
}

#[test]
fn toml_keys_are_covered_to_eleven_components() {
    // Eleven components, 10 dotted_keys deep, is the limit documented in
    // queries/toml.scm; the twelfth, leftmost, component falls back.
    for (text, highlight) in [
        (format!("[{}]\n", key(11)), "type"),
        (format!("[[{}]]\n", key(11)), "type"),
        (format!("{} = 1\n", key(11)), "property"),
    ] {
        let spans = highlighted("toml", &text);
        for i in 0..11 {
            assert!(
                has(&spans, &format!("k{i}"), highlight),
                "{text}: {spans:?}"
            );
        }
    }
    for text in [format!("[{}]\n", key(12)), format!("[[{}]]\n", key(12))] {
        let spans = highlighted("toml", &text);
        assert!(has(&spans, "k0", "property"), "{text}: {spans:?}");
        assert!(has(&spans, "k11", "type"), "{text}: {spans:?}");
    }
}

#[test]
fn json_and_yaml_keys_differ_from_values() {
    let json = highlighted("json", "{\"k\": \"v\"}");
    assert!(has(&json, "\"k\"", "string.special.key"), "{json:?}");
    assert!(has(&json, "\"v\"", "string"), "{json:?}");
    let yaml = highlighted("yaml", "k: v\n");
    assert!(has(&yaml, "k", "property"), "{yaml:?}");
    assert!(!has(&yaml, "v", "property"), "{yaml:?}");
}
