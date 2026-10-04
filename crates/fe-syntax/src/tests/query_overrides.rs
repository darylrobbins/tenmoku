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

/// A key of `n` components named `k0`, `k1`, …, each wrapped in `quote`.
fn key(n: usize, quote: &str) -> String {
    (0..n)
        .map(|i| format!("{quote}k{i}{quote}"))
        .collect::<Vec<_>>()
        .join(".")
}

/// The three places a key can appear, with the highlight a covered component
/// gets there and the one a component beyond the depth limit falls back to,
/// by quoting: bare keys fall back to `property`, quoted ones to `string`.
fn key_forms(key: &str) -> [(String, &'static str, [&'static str; 3]); 3] {
    [
        (
            format!("[{key}]\n"),
            "type",
            ["property", "string", "string"],
        ),
        (
            format!("[[{key}]]\n"),
            "type",
            ["property", "string", "string"],
        ),
        (
            format!("{key} = 1\n"),
            "property",
            ["property", "string", "string"],
        ),
    ]
}

#[test]
fn toml_keys_are_covered_to_eleven_components() {
    // Eleven components, 10 dotted_keys deep, is the limit documented in
    // queries/toml.scm; the twelfth, leftmost, component falls back.
    for (q, quote) in ["", "\"", "'"].into_iter().enumerate() {
        for (text, highlight, _) in key_forms(&key(11, quote)) {
            let spans = highlighted("toml", &text);
            for i in 0..11 {
                let component = format!("{quote}k{i}{quote}");
                assert!(has(&spans, &component, highlight), "{text}: {spans:?}");
            }
        }
        for (text, highlight, fallback) in key_forms(&key(12, quote)) {
            let spans = highlighted("toml", &text);
            let k0 = format!("{quote}k0{quote}");
            assert!(has(&spans, &k0, fallback[q]), "{text}: {spans:?}");
            let k11 = format!("{quote}k11{quote}");
            assert!(has(&spans, &k11, highlight), "{text}: {spans:?}");
        }
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
