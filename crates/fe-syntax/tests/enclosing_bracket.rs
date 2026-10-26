#![cfg(all(feature = "rust", feature = "javascript"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, enclosing_bracket_pair, language_by_name};

fn parse(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
}

fn enclosing(syntax: &Syntax, offset: usize) -> Option<(usize, usize)> {
    enclosing_bracket_pair(syntax, offset).map(|pair| (pair.open.start, pair.close.start))
}

#[test]
fn innermost() {
    let text = "f(a[1], {x: 2})";
    let s = parse("javascript", text);
    let at = |needle: &str| text.find(needle).unwrap();
    let braces = Some((at("{"), at("}")));
    let parens = Some((at("("), at(")")));
    assert_eq!(enclosing(&s, at("2")), braces);
    assert_eq!(enclosing(&s, at("a")), parens);
    assert_eq!(enclosing(&s, at("1")), Some((at("["), at("]"))));
    assert_eq!(enclosing(&s, 0), None);
    assert_eq!(enclosing(&s, text.len()), None);
}

#[test]
fn strings_and_comments_are_ignored() {
    let text = "fn f() { g(\"(\", 1) /* ) */ }";
    let s = parse("rust", text);
    let at = |needle: &str| text.find(needle).unwrap();
    // The `(` in the string isn't a bracket: `1` is in `g(..)`.
    assert_eq!(enclosing(&s, at("1")), Some((at("(\""), at(") /*"))));
    // Nor the `)` in the comment: after the call it's the braces.
    assert_eq!(enclosing(&s, at("/*")), Some((at("{"), at("}"))));
    // Inside the string, the call.
    assert_eq!(enclosing(&s, at("\"(") + 1), Some((at("(\""), at(") /*"))));
}
