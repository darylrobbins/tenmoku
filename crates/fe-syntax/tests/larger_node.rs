#![cfg(all(feature = "rust", feature = "python", feature = "json"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name, larger_syntax_node};

fn parse(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
}

/// Grow from `range` to the root, as the texts of each step.
fn chain<'a>(s: &Syntax, text: &'a str, mut range: std::ops::Range<usize>) -> Vec<&'a str> {
    let mut out = vec![];
    while let Some(next) = larger_syntax_node(s, range.clone()) {
        assert!(next.len() > range.len());
        out.push(&text[next.clone()]);
        range = next;
    }
    out
}

#[test]
fn rust_grows_to_root() {
    let text = "let x = foo(a, b + c);";
    let s = parse("rust", text);
    let at = text.find("b +").unwrap();
    let steps = chain(&s, text, at..at);
    assert_eq!(steps, ["b", "b + c", "(a, b + c)", "foo(a, b + c)", text]);
}

#[test]
fn caret_after_and_between_words() {
    let text = "a.b(c)";
    let s = parse("rust", text);
    let word = |at| larger_syntax_node(&s, at..at).map(|r| &text[r]);
    assert_eq!(word(1), Some("a"));
    assert_eq!(word(5), Some("c"));
    let text = "let x";
    let s = parse("rust", text);
    assert_eq!(larger_syntax_node(&s, 3..3), Some(0..3));
    assert_eq!(larger_syntax_node(&s, 4..4), Some(4..5));
}

#[test]
fn json_and_python() {
    let text = r#"{"a": [1, 2]}"#;
    let s = parse("json", text);
    let at = text.find('1').unwrap();
    assert_eq!(larger_syntax_node(&s, at..at + 1), Some(6..12));
    assert_eq!(larger_syntax_node(&s, 0..text.len()), None);
    let text = "def f(x):\n    return x + 1\n";
    let s = parse("python", text);
    let at = text.find("x +").unwrap();
    assert_eq!(&text[larger_syntax_node(&s, at..at + 1).unwrap()], "x + 1");
    let fresh = Syntax::new(
        language_by_name("python").unwrap(),
        &Buffer::new(text).snapshot(),
    );
    assert_eq!(larger_syntax_node(&fresh, 0..1), None);
}

#[test]
fn missing_nodes_are_skipped() {
    let text = "fn f( { let x = ; }";
    let s = parse("rust", text);
    for at in 0..=text.len() {
        let mut range = at..at;
        while let Some(next) = larger_syntax_node(&s, range.clone()) {
            assert!(!next.is_empty());
            assert!(next.len() > range.len() || range.is_empty());
            range = next;
        }
    }
}

#[test]
fn empty_document_has_nothing_larger() {
    let s = parse("rust", "");
    assert_eq!(larger_syntax_node(&s, 0..0), None);
    let s = parse("json", "");
    assert_eq!(larger_syntax_node(&s, 0..0), None);
}
