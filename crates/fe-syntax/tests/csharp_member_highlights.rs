//! C# member names colour as properties; a called method keeps its function colour.

#![cfg(feature = "csharp")]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

const SOURCE: &str = r#"[Obsolete(Message = "m")]
class A {
    public int Size { get; set; }
    private int _n;
    void M(A p) {
        var a = new A { Name = 1 };
        var b = a with { X = 2 };
        F(count: 3);
        this.field = p.Title;
        p.Run();
    }
}
"#;

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

fn is(name: &str, highlight: &str) -> bool {
    spans(SOURCE)
        .iter()
        .any(|(t, h)| t == name && *h == highlight)
}

#[test]
fn initializers_and_named_arguments_are_properties() {
    for name in ["Name", "X", "count", "Message"] {
        assert!(is(name, "property"), "{name}");
    }
}

#[test]
fn member_access_is_a_property_but_a_call_is_a_method() {
    assert!(is("field", "property"));
    assert!(is("Title", "property"));
    assert!(!is("Run", "property"));
    assert!(is("Run", "function.method") || is("Run", "function"));
}

#[test]
fn property_and_field_declarations_are_properties() {
    assert!(is("Size", "property"));
    assert!(is("_n", "property"));
}
