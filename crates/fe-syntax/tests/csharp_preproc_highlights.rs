//! C# preprocessor directives colour as keywords; their arguments stay plain.

#![cfg(feature = "csharp")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("csharp").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

fn keywords(text: &str) -> Vec<String> {
    spans(text)
        .into_iter()
        .filter(|(t, h)| *h == "keyword" && t.starts_with('#'))
        .map(|(t, _)| t)
        .collect()
}

#[test]
fn preprocessor_directives_are_keywords() {
    assert_eq!(
        keywords("#if DEBUG\nclass A {}\n#endif\n"),
        ["#if", "#endif"]
    );
}

#[test]
fn region_and_pragma_are_keywords() {
    assert_eq!(
        keywords("#region Foo\n#endregion\n"),
        ["#region", "#endregion"]
    );
    assert_eq!(keywords("#pragma warning disable\n"), ["#pragma"]);
    assert_eq!(keywords("#nullable enable\n"), ["#nullable"]);
}

#[test]
fn a_directive_inside_a_string_stays_a_string() {
    assert!(keywords("class A { string s = \"#if X\"; }\n").is_empty());
}
