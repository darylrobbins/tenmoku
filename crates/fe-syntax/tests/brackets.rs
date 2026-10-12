#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "json",
    feature = "javascript"
))]

use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_syntax::{BracketPair, Syntax, bracket_pair, language_by_name};

fn parse(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
}

fn pair(syntax: &Syntax, offset: usize) -> Option<(usize, usize)> {
    bracket_pair(syntax, offset).map(|BracketPair { open, close }| (open.start, close.start))
}

#[test]
fn rust_pairs() {
    let text = "fn f(a: [u8; 2]) { g(\"(\") }";
    let s = parse("rust", text);
    let at = |needle: &str| text.find(needle).unwrap();
    let paren = at("(a");
    let close_paren = at(") {");
    assert_eq!(pair(&s, paren), Some((paren, close_paren)));
    assert_eq!(pair(&s, close_paren), Some((paren, close_paren)));
    let open_sq = at("[u8");
    let close_sq = at("]");
    assert_eq!(pair(&s, close_sq), Some((open_sq, close_sq)));
    // After `]` a bracket starting there wins, here the `)`.
    assert_eq!(pair(&s, close_sq + 1), Some((paren, close_paren)));
    let brace = at("{");
    let close_brace = text.len() - 1;
    assert_eq!(pair(&s, brace), Some((brace, close_brace)));
    assert_eq!(pair(&s, text.len()), Some((brace, close_brace)));
    // The `(` in the string literal, and a non-bracket.
    assert_eq!(pair(&s, at("\"(") + 1), None);
    assert_eq!(pair(&s, at("fn") + 1), None);
    // Between `)(`.
    let s = parse("rust", "fn f() { (a)(b) }");
    assert_eq!(pair(&s, 12), Some((12, 14)));
    assert_eq!(pair(&s, 13), Some((12, 14)));
}

#[test]
fn recovery_and_other_languages() {
    let s = parse("python", "foo(bar(1), 2\n");
    assert_eq!(pair(&s, 3), None);
    assert_eq!(pair(&s, 7), Some((7, 9)));

    let s = parse("json", "{\"a\": [1, 2]}");
    assert_eq!(pair(&s, 6), Some((6, 11)));
    assert_eq!(pair(&s, 0), Some((0, 12)));

    let s = parse("javascript", "f(1, (2))");
    assert_eq!(pair(&s, 1), Some((1, 8)));
    assert_eq!(pair(&s, 5), Some((5, 7)));
}

#[test]
fn wide_parent_is_linear() {
    let numbers: Vec<String> = (0..200_000).map(|n| n.to_string()).collect();
    let text = format!("[{}]", numbers.join(","));
    let buffer = Buffer::new(&text);
    let snapshot = buffer.snapshot();
    // Best of several interleaved runs: a load spike hits one sample, not the
    // minimum of all of them, while quadratic pairing would fail every run.
    let mut parse_time = Duration::MAX;
    let mut pair_time = Duration::MAX;
    for _ in 0..5 {
        let mut syntax = Syntax::new(language_by_name("json").unwrap(), &snapshot);
        let started = Instant::now();
        syntax.finish(syntax.parse_job().run());
        parse_time = parse_time.min(started.elapsed());

        let started = Instant::now();
        let found = pair(&syntax, 0);
        pair_time = pair_time.min(started.elapsed());
        assert_eq!(found, Some((0, text.len() - 1)));
    }
    assert!(
        pair_time * 20 < parse_time,
        "pairing took {pair_time:?}, parsing {parse_time:?}"
    );
}
