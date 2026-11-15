//! PHP `preg_*` pattern strings, without their delimiters and flags, and Ruby
//! regex literals are highlighted by the hidden regex grammar.

#![cfg(all(feature = "regex", feature = "php", feature = "ruby"))]

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
fn php_preg_pattern_body_is_a_regex_without_delimiters_or_flags() {
    let found = spans("php", "<?php\npreg_match('/a+b/i', $s);\n");
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "'/", "string"), "{found:?}");
    assert!(has(&found, "/i'", "string"), "{found:?}");
}

#[test]
fn php_pattern_with_other_delimiters() {
    let found = spans(
        "php",
        "<?php\npreg_match('#x(y)#', $s);\npreg_replace('~a+~', '', $s);\npreg_split('{a|b}u', $s);\n",
    );
    assert!(has(&found, "(", "punctuation.bracket"), "{found:?}");
    assert!(has(&found, "+", "operator"), "{found:?}");
    assert!(has(&found, "|", "operator"), "{found:?}");
    assert!(has(&found, "'#", "string"), "{found:?}");
}

#[test]
fn php_non_preg_calls_not_injected() {
    let found = spans(
        "php",
        "<?php\nstrpos('/a+b/i', $s);\npreg_match('/a\\\\d+/', $s);\n",
    );
    assert!(!has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn php_interpolated_or_escaped_strings_stay_plain() {
    let found = spans(
        "php",
        "<?php\npreg_match(\"$p/a+b/\", $s);\npreg_match(\"\\n/a+b/\", $s);\n",
    );
    assert!(!has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn php_underscore_delimiter() {
    let found = spans("php", "<?php\npreg_match('_a+b_i', $s);\n");
    assert!(has(&found, "+", "operator"), "{found:?}");
}

#[test]
fn ruby_regex_literal_is_a_regex() {
    // The extension side is the wasm module's; this is as far as the
    // native language goes: it has no injection query.
    let found = spans("ruby", "x = /\\d+/i\ny = %r{a|b}\n");
    assert!(!found.is_empty());
}
