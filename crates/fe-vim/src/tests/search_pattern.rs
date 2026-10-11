//! Search patterns (`search.rs`). Expected matches were recorded with
//! `nvim --clean` (0.12.4): each row's matches by repeating
//! `searchpos(pat, 'nW', row)` from the row's start (the first with `c`),
//! their lengths from `matchbufline()`, and where `/` and `?` go from a
//! cursor by `searchpos(pat, 'nw')` and `searchpos(pat, 'nbw')`.

// One match is a list of one range here, not a range to collect.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_text::Text;

use crate::search::{Pattern, PatternError, ROWS_SEARCHED, matches_in};

fn parse(pattern: &str) -> Pattern {
    Pattern::parse(pattern).unwrap_or_else(|error| panic!("{pattern}: {error}"))
}

/// Every match in a one-row `text`.
fn all(pattern: &str, text: &str) -> Vec<Range<usize>> {
    matches_in(&Text::new(text), 0..1, &parse(pattern), usize::MAX)
}

#[test]
fn translates_or_fails() {
    // (pattern, row, nvim's matches)
    let cases: &[(&str, &str, &[Range<usize>])] = &[
        (
            "foo\\|bar",
            "foo bar foobar baz",
            &[0..3, 4..7, 8..11, 11..14],
        ),
        ("\\<is\\>", "this is his island is", &[5..7, 19..21]),
        (
            "a\\{2,3}",
            "a aa aaa aaaa aaaaa",
            &[2..4, 5..8, 9..12, 14..17, 17..19],
        ),
        ("a\\{-1,}", "xaaab aa", &[1..2, 2..3, 3..4, 6..7, 7..8]),
        ("\\(ab\\)\\+", "xababc ab abab", &[1..5, 7..9, 10..14]),
        ("\\%(ab\\)c", "abc ababc ac", &[0..3, 6..9]),
        ("[^a-c]x", "ax bx dx cx ex", &[6..8, 12..14]),
        ("\\d\\+", "ab 123 4 x56", &[3..6, 7..8, 10..12]),
        ("\\v(a|b)+", "xabbac b", &[1..5, 7..8]),
        // `\V`: the dot is itself.
        ("\\Vf.o", "foo f.o fxo", &[4..7]),
        ("\\cFOO", "foo FoO FOO fo", &[0..3, 4..7, 8..11]),
        ("x\\=y", "xy y xxy", &[0..2, 3..4, 6..8]),
        // `^` and `$` only at a branch's ends, `*` with nothing before it:
        // otherwise they're themselves.
        ("^a\\|a^b", "a a^b", &[0..1, 2..5]),
        ("*a\\|b$", "*a b$ b", &[0..2, 6..7]),
        ("a$b", "a$b", &[0..3]),
        // A `]` first may start a range.
        ("[]-a]", "-^", &[1..2]),
        // In a collection, a backslash before what isn't a code is itself.
        ("a[\\é]\\+b", "a\\éb", &[0..5]),
    ];
    for (pattern, text, expected) in cases {
        assert_eq!(all(pattern, text), *expected, "{pattern} in {text:?}");
    }
    // `\c` folds as nvim does (nvim: `match()` of each against each).
    let kelvin = "\u{212a}";
    let long_s = "\u{17f}";
    let folds = format!("k K {kelvin} s S {long_s} ß ẞ σ ς");
    let found = |pattern: &str| -> Vec<&str> {
        all(pattern, &folds)
            .iter()
            .map(|range| &folds[range.clone()])
            .collect()
    };
    assert_eq!(found("\\ck"), ["k", "K", kelvin]);
    assert_eq!(found("\\c\\V\u{212a}"), ["k", "K", kelvin]);
    assert_eq!(found("\\cS"), ["s", "S", long_s]);
    assert_eq!(found("\\cß"), ["ß"]);
    assert_eq!(found("\\cẞ"), ["ẞ"]);
    assert_eq!(found("\\cσ"), ["σ", "ς"]);
    assert_eq!(found("\\c[ß]"), ["ß"]);
    // `[a-z]` spells out `\l`, which folds within ASCII; `[a-y]` doesn't.
    assert_eq!(found("\\c[a-z]"), ["k", "K", "s", "S"]);
    assert_eq!(found("\\c[a-y]"), ["k", "K", kelvin, "s", "S", long_s]);
    assert_eq!(found("\\c[a-zk]"), ["k", "K", kelvin, "s", "S", long_s]);
    // With a space it's no longer `\l`, so it folds fully.
    assert_eq!(found("\\c[^a-z ]"), ["ß", "ẞ", "σ", "ς"]);
    assert_eq!(all("\\c[^a-z]", "k\u{212a}s\u{17f}"), [1..4, 5..7]);
    assert_eq!(found("\\c[^k ]"), ["s", "S", long_s, "ß", "ẞ", "σ", "ς"]);
    assert_eq!(found("[a-y]"), ["k", "s"]);
    // Only plain `a-z` with no `[:name:]` class spells out `\l`.
    assert_eq!(found("\\c[\\x61-z]"), ["k", "K", kelvin, "s", "S", long_s]);
    assert_eq!(
        found("\\c[a-z[:digit:]]"),
        ["k", "K", kelvin, "s", "S", long_s]
    );

    // An empty match moves on by a whole character, composing characters
    // and emoji sequences included (nvim: `x*` in "ae\u{301}b" and
    // "a\u{1f468}\u{200d}\u{1f469}b").
    assert_eq!(all("x*", "ae\u{301}b"), [0..0, 1..1, 4..4]);
    assert_eq!(
        all("x*", "a\u{1f468}\u{200d}\u{1f469}b"),
        [0..0, 1..1, 12..12]
    );
    // A control character takes the nonspacing marks after it (nvim: `x*`
    // in "\t\u{301}a").
    assert_eq!(all("x*", "\t\u{301}a"), [0..0, 3..3]);
    // `/` starts a whole character on, even from inside one (nvim: `x*`
    // from columns 1 and 5 of "a\u{1f1e8}\u{1f1e6}\u{1f1fa}\u{1f1f8}b").
    let flags = Text::new("a\u{1f1e8}\u{1f1e6}\u{1f1fa}\u{1f1f8}b");
    assert_eq!(parse("x*").find(&flags, 1, true), Some(9..9));
    assert_eq!(parse("x*").find(&flags, 5, true), Some(17..17));
    // A match ending on the last character is found, and `/` from that
    // character goes on at the next row (nvim: `b.*` and `$` in
    // "abe\u{301}", `$` from columns 0 and 2).
    let text = Text::new("abe\u{301}\nc");
    assert_eq!(parse("b.*").find(&text, 0, true), Some(1..5));
    assert_eq!(parse("$").find(&text, 0, true), Some(5..5));
    assert_eq!(parse("$").find(&text, 2, true), Some(7..7));

    // nvim has no last substitute string for `~` (E33); fe-vim takes it as
    // itself.
    assert_eq!(all("a~b", "ab a~b"), [3..6]);

    for (pattern, atom) in [
        ("a\\zsb", "\\zs"),
        ("a\\_sb", "\\_s"),
        ("a\\nb", "\\n"),
        ("\\%Va", "\\%V"),
        ("\\(a\\)\\1", "\\1"),
        ("a\\é", "\\é"),
        // Range ends Vim reads its own way.
        ("[%-\\]]", "%-\\]"),
        ("[!--]", "!--"),
    ] {
        let error = Pattern::parse(pattern).unwrap_err();
        assert_eq!(error, PatternError::Unsupported(atom.into()), "{pattern}");
        assert_eq!(error.to_string(), format!("pattern not supported: {atom}"));
    }

    // Over `regex`'s size limit, and nested deeper than it goes.
    assert!(matches!(
        Pattern::parse("\\(a\\{1000}\\)\\{1000}"),
        Err(PatternError::TooLarge(_))
    ));
    let nested = format!("{}a{}", "\\%(".repeat(300), "\\)".repeat(300));
    assert!(matches!(
        Pattern::parse(&nested),
        Err(PatternError::TooLarge(_))
    ));
}

#[test]
fn find_is_row_bounded() {
    let find = |pattern: &str, text: &str, from: usize, forward: bool| {
        parse(pattern).find(&Text::new(text), from, forward)
    };
    // Nothing reaches past a row's end.
    assert_eq!(find("a.*", "ab\nab", 0, false), Some(3..5));
    assert_eq!(find("a\\s*", "a  \n  b", 3, true), Some(0..3));
    assert_eq!(find("[^x]\\+", "ab\ncd", 0, true), Some(3..5));
    // A match at `from` is skipped going forward, and the search wraps
    // once past either end (nvim: "abababababab" with `abab`, from 0, 4
    // and 11).
    let text = "abababababab";
    assert_eq!(find("abab", text, 0, true), Some(4..8));
    assert_eq!(find("abab", text, 11, true), Some(0..4));
    assert_eq!(find("abab", text, 0, false), Some(8..12));
    assert_eq!(find("abab", text, 4, false), Some(0..4));
    // Its only match, from itself, either way.
    assert_eq!(find("b\\+", "a\nbb\nc", 3, true), Some(2..4));
    assert_eq!(find("b\\+", "a\nbb\nc", 2, false), Some(2..4));
    // An empty match moves on one position, and none is taken at a row's
    // end once past its start (nvim: `x*` in "abc" and "de").
    let text = "abc\nde";
    assert_eq!(find("x*", text, 0, true), Some(1..1));
    assert_eq!(find("x*", text, 2, true), Some(4..4));
    assert_eq!(find("x*", text, 0, false), Some(5..5));
    // Forward from a row's last character, its end is taken only after
    // wrapping (nvim: `$` from columns 1 and 2 of "abc").
    assert_eq!(find("$", text, 1, true), Some(3..3));
    assert_eq!(find("$", text, 2, true), Some(6..6));

    let rows = 200_000;
    let text = Text::new(&"abc\n".repeat(rows));
    let pattern = parse("abd");
    for forward in [true, false] {
        ROWS_SEARCHED.set(0);
        assert_eq!(pattern.find(&text, text.len() / 2, forward), None);
        assert!(ROWS_SEARCHED.get() <= rows + 1, "{}", ROWS_SEARCHED.get());
    }
}

#[test]
fn matches_in_is_capped() {
    let text = Text::new("ab ab\nxx\nab\nab");
    let pattern = parse("ab");
    assert_eq!(matches_in(&text, 0..3, &pattern, 10), [0..2, 3..5, 9..11]);
    assert_eq!(matches_in(&text, 2..9, &pattern, 10), [9..11, 12..14]);
    assert_eq!(matches_in(&text, 0..4, &pattern, 2), [0..2, 3..5]);

    // A row of 100,000 matches (spread over the rope's chunks).
    let text = Text::new(&format!("x\n{}", "a".repeat(100_000)));
    let found = matches_in(&text, 0..2, &parse("a"), 1_000);
    assert_eq!(found.len(), 1_000);
    assert_eq!(found[0], 2..3);
    assert_eq!(found[999], 1001..1002);

    // Zero-width matches are empty ranges (nvim: `\<` in "ab cd", `x*` in
    // "ab").
    assert_eq!(all("\\<", "ab cd"), [0..0, 3..3]);
    assert_eq!(all("x*", "ab"), [0..0, 1..1]);
}

#[test]
fn word_bounds_join_quantified_and_anchored_patterns() {
    // A greedy or lazy quantifier gives up characters to meet the boundary.
    assert_eq!(all("\\<foo.*\\>", "foo bar!"), [0..7]);
    assert_eq!(all("\\<foo.\\{-}\\>", "foo bar!"), [0..3]);
    // `^` and `$` beside a boundary are still plain characters.
    assert_eq!(all("\\<^foo\\>", "foo"), Vec::<Range<usize>>::new());
    assert_eq!(all("\\<foo$\\>", "foo"), Vec::<Range<usize>>::new());
}

#[test]
fn huge_counts_of_mixed_groups_finish() {
    // Each is written out one time after another or not at all: none may
    // run away or overflow the stack, and each is either a pattern or too
    // large.
    for count in ["1000000", "0,1000000", "1000000,", "-1000000", "-0,1000000"] {
        let pattern = format!("\\%(a\\|.\\)\\{{{count}}}");
        let result = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || Pattern::parse(&pattern).map(|_| ()))
            .unwrap()
            .join()
            .expect("no overflow");
        assert!(
            matches!(result, Ok(()) | Err(PatternError::TooLarge(_))),
            "{count}: {result:?}"
        );
    }
}
