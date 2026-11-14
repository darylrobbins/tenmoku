//! XML attribute names colour as attributes, not as plain properties.

#![cfg(feature = "xml")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("xml").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

fn has(spans: &[(String, &'static str)], text: &str, name: &str) -> bool {
    spans.iter().any(|(t, h)| t == text && *h == name)
}

#[test]
fn attribute_names_are_attributes() {
    let s = spans("<a href=\"x\" id='y'>t</a>");
    assert!(has(&s, "href", "attribute"), "{s:?}");
    assert!(has(&s, "id", "attribute"), "{s:?}");
    assert!(has(&s, "x", "string"), "{s:?}");
    assert!(has(&s, "y", "string"), "{s:?}");
}

#[test]
fn xml_declaration_pseudo_attributes_are_attributes() {
    let s = spans("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    assert!(has(&s, "version", "attribute"), "{s:?}");
    assert!(has(&s, "encoding", "attribute"), "{s:?}");
}
