//! Java fields, field declarations and annotation keys colour as properties.

#![cfg(feature = "java")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("java").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    syntax
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

fn has(all: &[(String, &'static str)], text: &str, highlight: &str) -> bool {
    all.contains(&(text.to_string(), highlight))
}

#[test]
fn field_access_is_a_property_and_a_call_is_a_method() {
    let all = spans(
        "class A {\n    void f() {\n        x = this.count + a.b.size;\n        a.run();\n    }\n}\n",
    );
    assert!(has(&all, "count", "property"), "{all:?}");
    assert!(has(&all, "size", "property"), "{all:?}");
    assert!(has(&all, "run", "function.method"), "{all:?}");
    assert!(!has(&all, "run", "property"), "{all:?}");
}

#[test]
fn field_declarations_are_properties() {
    let all = spans("class A {\n    private int count = 0;\n    String a, b;\n}\n");
    for name in ["count", "a", "b"] {
        assert!(has(&all, name, "property"), "{name}: {all:?}");
    }
}

#[test]
fn annotation_keys_are_properties() {
    let all = spans("class A {\n    @Test(timeout = 5)\n    void f() {}\n}\n");
    assert!(has(&all, "timeout", "property"), "{all:?}");
}
