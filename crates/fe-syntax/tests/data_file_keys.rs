//! Keys of data files (YAML, TOML, INI, JSON, JSON5) are `string.special.key`;
//! their values, table headers and code properties are not.

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text` in the language `name`.
fn spans(name: &str, text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name(name).unwrap(), Buffer::new(text).current());
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

const KEY: &str = "string.special.key";

#[test]
fn data_file_keys_are_string_special_key() {
    let cases: &[(&str, &str, &[&str], &[&str])] = &[
        (
            "yaml",
            "a: 1\n\"b\": 2\n'c': 3\nd: {e: 1, \"f\": 2}\n",
            &["a", "\"b\"", "'c'", "d", "e", "\"f\""],
            &["1", "2", "3"],
        ),
        (
            "toml",
            "x = 1\nd.e = 2\n\"q\" = 3\n",
            &["x", "d", "e", "\"q\""],
            &["1", "2", "3"],
        ),
        ("ini", "[s]\nname = value\n", &["name"], &["value"]),
        ("json", "{\"k\": \"v\"}", &["\"k\""], &["\"v\""]),
        (
            "json5",
            "{k: \"v\", \"m\": 1}",
            &["k", "\"m\""],
            &["\"v\"", "1"],
        ),
    ];
    for &(lang, text, keys, values) in cases {
        let found = spans(lang, text);
        for key in keys {
            assert!(has(&found, key, KEY), "{lang} {key}: {found:?}");
        }
        for value in values {
            assert!(!has(&found, value, KEY), "{lang} {value}: {found:?}");
        }
    }
}

#[test]
fn headers_and_code_fields_keep_their_highlights() {
    let toml = spans("toml", "[a.b]\n[[bin]]\n");
    for name in ["a", "b", "bin"] {
        assert!(has(&toml, name, "type"), "{name}: {toml:?}");
    }
    let ini = spans("ini", "[section]\nk = v\n");
    assert!(has(&ini, "section", "type"), "{ini:?}");
    // Distinct names, so each occurrence is asserted on its own.
    let rust = spans(
        "rust",
        "struct S { defd: u8 }\nfn g(s: S) -> u8 { s.used }\n",
    );
    assert!(has(&rust, "defd", "property"), "{rust:?}");
    assert!(has(&rust, "used", "property"), "{rust:?}");
    assert!(!rust.iter().any(|(_, h)| *h == KEY), "{rust:?}");
    let css = spans("css", "a { color: red; }\n");
    assert!(has(&css, "color", "property"), "{css:?}");
    let env = spans("dotenv", "NAME=1\n");
    assert!(has(&env, "NAME", "property"), "{env:?}");
    let git = spans("gitconfig", "[user]\n\tname = x\n");
    assert!(has(&git, "name", "property"), "{git:?}");
}

#[test]
fn yaml_scalar_keys_of_any_type_are_keys_and_values_keep_theirs() {
    let block = spans("yaml", "123: 456\ntrue: false\nnull: null\n1.5: 2.5\n");
    for key in ["123", "true", "1.5"] {
        assert!(has(&block, key, KEY), "{key}: {block:?}");
    }
    assert!(has(&block, "456", "number"), "{block:?}");
    assert!(has(&block, "2.5", "number"), "{block:?}");
    assert!(has(&block, "false", "constant.builtin"), "{block:?}");
    assert!(has(&block, "null", KEY), "{block:?}");
    assert!(has(&block, "null", "constant.builtin"), "{block:?}");
    let flow = spans("yaml", "{7: 8, true: false, null: x}\n");
    for key in ["7", "true", "null"] {
        assert!(has(&flow, key, KEY), "{key}: {flow:?}");
    }
    assert!(has(&flow, "8", "number"), "{flow:?}");
    assert!(has(&flow, "false", "constant.builtin"), "{flow:?}");
}
