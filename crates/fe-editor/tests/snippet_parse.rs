//! `Snippet::parse`: the LSP snippet grammar, its text with placeholders
//! filled and each tabstop's ranges in it, in visiting order.

// A stop's ranges are a `Vec`, often of one.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_editor::snippet::{Snippet, StopId};

/// Each tabstop's id and ranges, in visiting order.
fn stops(snippet: &Snippet) -> Vec<(StopId, Vec<Range<usize>>)> {
    snippet
        .tabstops()
        .iter()
        .map(|stop| (stop.id.clone(), stop.ranges.clone()))
        .collect()
}

fn n(number: u32) -> StopId {
    StopId::Number(number)
}

fn var(name: &str) -> StopId {
    StopId::Variable(name.into())
}

#[test]
fn parses_tabstops_placeholders_choices_and_mirrors() {
    let snippet = Snippet::parse("fn ${1:name}(${2:arg}: ${3|i32,u64|}) {\n\t$0\n}");
    assert_eq!(snippet.text(), "fn name(arg: i32) {\n\t\n}");
    assert_eq!(
        stops(&snippet),
        [
            (n(1), vec![3..7]),
            (n(2), vec![8..11]),
            (n(3), vec![13..16]),
            (n(0), vec![21..21]),
        ]
    );
    assert_eq!(snippet.tabstops()[2].choices, ["i32", "u64"]);
    assert!(snippet.tabstops()[0].choices.is_empty());

    // A repeated number mirrors the placeholder, wherever it is; `${n}`
    // is `$n`, and the stops are visited by number.
    let snippet = Snippet::parse("$2 $1 ${1:a} ${1} ${2:bc}");
    assert_eq!(snippet.text(), "bc a a a bc");
    assert_eq!(
        stops(&snippet),
        [(n(1), vec![3..4, 5..6, 7..8]), (n(2), vec![0..2, 9..11])]
    );

    // A transform mirrors, untransformed.
    let snippet = Snippet::parse("${1:a} ${1/(.*)/${1:/upcase}/g}!");
    assert_eq!(snippet.text(), "a a!");
    assert_eq!(stops(&snippet), [(n(1), vec![0..1, 2..3])]);

    // `${0}` is the final stop, visited last; a tabstop with no
    // placeholder is empty.
    let snippet = Snippet::parse("${0}x$10$3");
    assert_eq!(snippet.text(), "x");
    assert_eq!(
        stops(&snippet),
        [(n(3), vec![1..1]), (n(10), vec![1..1]), (n(0), vec![0..0])]
    );
}

#[test]
fn nested_placeholders_and_escapes() {
    let snippet = Snippet::parse("${1:outer ${2:inner ${3:most}} end}$0");
    assert_eq!(snippet.text(), "outer inner most end");
    assert_eq!(
        stops(&snippet),
        [
            (n(1), vec![0..20]),
            (n(2), vec![6..16]),
            (n(3), vec![12..16]),
            (n(0), vec![20..20]),
        ]
    );

    // A mirror shows the placeholder's text, but the stops inside it are
    // only in the placeholder.
    let snippet = Snippet::parse("${1:a${2:b}} $1");
    assert_eq!(snippet.text(), "ab ab");
    assert_eq!(
        stops(&snippet),
        [(n(1), vec![0..2, 3..5]), (n(2), vec![1..2])]
    );

    // A placeholder mirroring itself is cut short rather than endless.
    assert_eq!(Snippet::parse("${1:a$1}").text(), "a");

    let snippet = Snippet::parse(r"\$1 \} \\ \x ${1:a\}b} }");
    assert_eq!(snippet.text(), r"$1 } \ \x a}b }");
    assert_eq!(stops(&snippet), [(n(1), vec![10..13])]);

    let snippet = Snippet::parse(r"${1|a\,b,c\|d,\$\}\\|}");
    assert_eq!(snippet.text(), "a,b");
    assert_eq!(snippet.tabstops()[0].choices, ["a,b", "c|d", r"$}\"]);

    // Multi-byte text keeps byte ranges.
    let snippet = Snippet::parse("é${1:ü}ß");
    assert_eq!(snippet.text(), "éüß");
    assert_eq!(stops(&snippet), [(n(1), vec![2..4])]);
}

#[test]
fn unknown_variables_become_placeholders_or_their_default() {
    let snippet = Snippet::parse("$TM_FILENAME ${CLIPBOARD:paste} ${UNKNOWN} $1 $0");
    assert_eq!(snippet.text(), "TM_FILENAME paste UNKNOWN  ");
    assert_eq!(
        stops(&snippet),
        [
            (n(1), vec![26..26]),
            (var("TM_FILENAME"), vec![0..11]),
            (var("UNKNOWN"), vec![18..25]),
            (n(0), vec![27..27]),
        ]
    );

    // A default may hold stops; a variable's transform is dropped. Each
    // variable is a stop of its own, in order.
    let snippet = Snippet::parse("${NAME:${1:x}} ${_A/a/b/} $_A");
    assert_eq!(snippet.text(), "x _A _A");
    assert_eq!(
        stops(&snippet),
        [
            (n(1), vec![0..1]),
            (var("_A"), vec![2..4]),
            (var("_A"), vec![5..7]),
        ]
    );
}

#[test]
fn malformed_input_is_literal_text() {
    for source in [
        "${1:unclosed",
        "${",
        "$",
        "${}",
        "${1|a,b}",
        "${1|a,b|",
        "${foo bar}",
        "${1/unclosed/",
        "${1x}",
        "$-1 $ $$",
        "}",
    ] {
        let snippet = Snippet::parse(source);
        assert_eq!(snippet.text(), source, "{source}");
        assert!(snippet.tabstops().is_empty(), "{source}");
    }

    // What's well formed inside something that isn't still parses.
    let snippet = Snippet::parse("${1:a ${2:b}");
    assert_eq!(snippet.text(), "${1:a b");
    assert_eq!(stops(&snippet), [(n(2), vec![6..7])]);

    // Deeply nested placeholders don't overflow the stack, nor take long
    // when none is closed.
    let deep = "${1:".repeat(10_000);
    assert_eq!(Snippet::parse(&deep).text(), deep);
    let closed = format!("{}x{}", "${A:".repeat(10_000), "}".repeat(10_000));
    assert!(Snippet::parse(&closed).text().contains('x'));
}

#[test]
fn hostile_input_is_bounded() {
    use fe_editor::snippet::MAX_MIRROR_BYTES;

    // A long chain of placeholders each mirroring the next.
    let chain: String = (1..100_000)
        .map(|n| format!("${{{n}:${}}}", n + 1))
        .collect();
    let snippet = Snippet::parse(&chain);
    assert_eq!(snippet.tabstops().len(), 100_000);

    // Mirrors that double at each step stop growing.
    let mut doubling: String = (1..31)
        .map(|n| format!("${{{n}:${}${}}}", n + 1, n + 1))
        .collect();
    doubling.push_str("${31:x}");
    let snippet = Snippet::parse(&doubling);
    assert!(snippet.text().len() <= doubling.len() + MAX_MIRROR_BYTES);

    // Many placeholders sharing one large one's text, and many mirrors of
    // it past the limit.
    let large = "a".repeat(1 << 20);
    let mut shared = format!("${{1:{large}}}");
    for n in 2..2_000 {
        shared.push_str(&format!("${{{n}:$1}}${n}"));
    }
    let snippet = Snippet::parse(&shared);
    assert!(snippet.text().len() <= shared.len() + MAX_MIRROR_BYTES);
    let many = format!("${{1:{}}}{}", "a".repeat(5 << 20), "$1".repeat(100_000));
    assert!(Snippet::parse(&many).text().len() <= many.len() + MAX_MIRROR_BYTES);

    // Unclosed transforms and choices, each reparsed inside the ones
    // before it, are literal text.
    for unit in ["${1/", "${1|a,", "${1:${2/a/${"] {
        let source = unit.repeat(20_000);
        let snippet = Snippet::parse(&source);
        assert_eq!(snippet.text(), source, "{unit}");
    }
}
