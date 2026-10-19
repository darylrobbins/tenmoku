#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "json",
    feature = "javascript"
))]

use std::time::{Duration, Instant};

use fe_buffer::{Buffer, Origin};
use fe_syntax::{
    BracketDepth, BracketPair, Syntax, bracket_depths, bracket_pair, language_by_name,
};

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

/// Each paired bracket in `range` as its text and depth.
fn depths(syntax: &Syntax, text: &str, range: std::ops::Range<usize>) -> Vec<(usize, usize)> {
    bracket_depths(syntax, range)
        .into_iter()
        .map(|BracketDepth { range, depth }| {
            assert_eq!(range.len(), 1, "{:?}", &text[range.clone()]);
            (range.start, depth)
        })
        .collect()
}

#[test]
fn depths_nest_and_skip_strings_and_comments() {
    let text = "fn f(a: [u8; 2]) { g(\"([{\", (b)) // ({[\n}";
    let s = parse("rust", text);
    let at = |needle: &str| text.find(needle).unwrap();
    let close_g = at(")) //") + 1;
    let expected = [
        (at("(a"), 0),
        (at("[u8"), 1),
        (at("]"), 1),
        (at(") {"), 0),
        (at("{ g"), 0),
        (at("(\""), 1),
        (at("(b"), 2),
        (at("b)") + 1, 2),
        (close_g, 1),
        (text.len() - 1, 0),
    ];
    assert_eq!(depths(&s, text, 0..text.len()), expected);

    // A range in the middle has the same depths, and only its brackets.
    let from = at("(b");
    assert_eq!(depths(&s, text, from..close_g), expected[6..8]);
    assert_eq!(depths(&s, text, 0..0), []);
}

#[test]
fn depths_leave_out_unpaired_and_wide_nodes_pair_their_ends() {
    // `foo(` is never closed: only `bar`'s pair.
    let text = "foo(bar(1), 2\n";
    let s = parse("python", text);
    assert_eq!(depths(&s, text, 0..text.len()), [(7, 0), (9, 0)]);

    let items = vec!["[1]"; 5000].join(", ");
    let text = format!("[{items}]");
    let s = parse("json", &text);
    let tail = text.len() - 4;
    assert_eq!(
        depths(&s, &text, tail..text.len()),
        [(tail, 1), (tail + 2, 1), (tail + 3, 0)]
    );
}

/// An edited tree not yet reparsed has its leaves moved, not re-lexed: a
/// bracket that new quotes enclose is still a leaf. No depths until the
/// reparse, which then leaves the quoted brackets out.
#[test]
fn depths_wait_for_a_reparse() {
    let text = "[1, [2] , [3]]";
    let mut buffer = Buffer::new(text);
    let mut s = Syntax::new(language_by_name("json").unwrap(), &buffer.snapshot());
    s.finish(s.parse_job().run());
    let inner = text.find("[2]").unwrap();
    // A quote before `[2]` and one after, each synced into the tree.
    for at in [inner, inner + 5] {
        buffer.edit([(at..at, "\"")], Origin::User);
        s.edit(&buffer.snapshot());
        assert!(s.needs_parse());
        assert_eq!(bracket_depths(&s, 0..buffer.snapshot().len()), []);
    }
    let edited = buffer.snapshot().text().to_string();
    assert_eq!(edited, "[1, \"[2] \", [3]]");
    s.finish(s.parse_job().run());
    assert!(!s.needs_parse());
    let at = |needle: &str| edited.find(needle).unwrap();
    assert_eq!(
        depths(&s, &edited, 0..edited.len()),
        [
            (0, 0),
            (at("[3"), 1),
            (at("3]") + 1, 1),
            (edited.len() - 1, 0)
        ]
    );
}

/// Go's grouped declarations are wide nodes with a keyword, and maybe
/// comments, before the opening bracket: `const (` and `type (`.
#[cfg(feature = "go")]
#[test]
fn depths_in_wide_go_groups() {
    let comments = "/* c */ ".repeat(20);
    let specs: String = (0..5000).map(|i| format!("\tA{i} = (1 + (2))\n")).collect();
    let text = format!("package p\n\nconst {comments}(\n{specs})\n");
    let s = parse("go", &text);
    let group = text.find(&format!("{comments}(")).unwrap() + comments.len();
    let close = text.len() - 2;
    // The whole file, and only the last spec.
    let all = depths(&s, &text, 0..text.len());
    assert_eq!(all.len(), 2 + 5000 * 4);
    assert_eq!(all[..3], [(group, 0), (group + 8, 1), (group + 13, 2)]);
    assert_eq!(all[all.len() - 1], (close, 0));
    let tail = text.rfind("\tA4999").unwrap();
    let open = tail + "\tA4999 = ".len();
    assert_eq!(
        depths(&s, &text, tail..text.len()),
        [
            (open, 1),
            (open + 5, 2),
            (open + 7, 2),
            (open + 8, 1),
            (close, 0)
        ]
    );
    assert_eq!(depths(&s, &text, 0..group + 1), [(group, 0)]);

    let specs: String = (0..5000)
        .map(|i| format!("\tT{i} [2]func(int)\n"))
        .collect();
    let text = format!("package p\n\ntype {comments}(\n{specs})\n");
    let s = parse("go", &text);
    let group = text.find(&format!("{comments}(")).unwrap() + comments.len();
    let all = depths(&s, &text, 0..text.len());
    assert_eq!(all.len(), 2 + 5000 * 4);
    assert_eq!(all[0], (group, 0));
    assert_eq!(all[1].1, 1);
    let tail = text.rfind("\tT4999").unwrap();
    let at = |needle: &str| tail + text[tail..].find(needle).unwrap();
    assert_eq!(
        depths(&s, &text, tail..text.len()),
        [
            (at("["), 1),
            (at("]"), 1),
            (at("("), 1),
            (at(")"), 1),
            (text.len() - 2, 0)
        ]
    );
}

/// A range starting on a blank line, where tree-sitter's seek to the first
/// child at a byte finds none in Go, still has the brackets after it.
#[cfg(feature = "go")]
#[test]
fn depths_from_a_blank_line() {
    let text = "package p\n\nvar x = f(1)\n";
    let s = parse("go", text);
    let open = text.find('(').unwrap();
    assert_eq!(depths(&s, text, 10..text.len()), [(open, 0), (open + 2, 0)]);

    // Blank lines between a group's specs.
    let specs: String = (0..10).map(|i| format!("\tA{i} = (1 + (2))\n\n")).collect();
    let text = format!("package p\n\nconst (\n{specs})\n");
    let s = parse("go", &text);
    let blank = text.find("\n\n\tA5").unwrap() + 1;
    let spec = blank + "\n\tA5 = ".len();
    let close = text.len() - 2;
    let found = depths(&s, &text, blank..text.len());
    assert_eq!(found[..2], [(spec, 1), (spec + 5, 2)]);
    assert_eq!(found.len(), 5 * 4 + 1);
    assert_eq!(found[found.len() - 1], (close, 0));
}

/// Wide nodes with a named child before the opening bracket: a Rust
/// `macro_rules!` definition's name, a C# switch expression's subject.
#[cfg(feature = "csharp")]
#[test]
fn depths_in_wide_nodes_after_a_name() {
    let rules: String = (0..5000).map(|_| "\t(a) => { f([1]) };\n").collect();
    let text = format!("macro_rules! m {{\n{rules}}}\n");
    let s = parse("rust", &text);
    let brace = text.find('{').unwrap();
    let close = text.len() - 2;
    let rule = [1, 1, 1, 2, 3, 3, 2, 1];
    let all = depths(&s, &text, 0..text.len());
    assert_eq!(all.len(), 2 + 5000 * rule.len());
    assert_eq!(all[0], (brace, 0));
    assert_eq!(all[all.len() - 1], (close, 0));
    let inner: Vec<usize> = all[1..all.len() - 1].iter().map(|&(_, d)| d).collect();
    assert!(inner.chunks(rule.len()).all(|chunk| chunk == rule));
    // The last rule alone, and the first.
    let tail = text.rfind("\t(a)").unwrap();
    let tail_depths: Vec<usize> = depths(&s, &text, tail..text.len())
        .iter()
        .map(|&(_, d)| d)
        .collect();
    assert_eq!(tail_depths, [&rule[..], &[0]].concat());
    assert_eq!(
        depths(&s, &text, 0..brace + 5),
        [(brace, 0), (brace + 3, 1)]
    );

    let arms: String = (0..5000).map(|i| format!("\t{i} => ((1)),\n")).collect();
    let text = format!("class C {{ int M(int x) => x switch {{\n{arms}\t_ => (2) }}; }}\n");
    let s = parse("csharp", &text);
    let switch = text.find("switch {").unwrap() + "switch ".len();
    let close = text.rfind("};").unwrap();
    let all = depths(&s, &text, 0..text.len());
    // The class's pair, `M(…)`, the switch's, and each arm's two.
    assert_eq!(all.len(), 2 + 2 + 2 + 5000 * 4 + 2);
    assert!(all.contains(&(switch, 1)) && all.contains(&(close, 1)));
    let tail = text.rfind("\t4999").unwrap();
    let arm = tail + "\t4999 => ".len();
    assert_eq!(
        depths(&s, &text, tail..text.len())[..5],
        [
            (arm, 2),
            (arm + 1, 3),
            (arm + 3, 3),
            (arm + 4, 2),
            (text.rfind("(2)").unwrap(), 2)
        ]
    );
    assert_eq!(
        depths(&s, &text, tail..text.len())[6..],
        [(close, 1), (text.len() - 2, 0)]
    );
}

/// A wide node's pairing is kept for its tree: later calls don't step
/// through its children again, and a reparse pairs the new tree.
#[test]
fn wide_pairing_is_kept_until_the_reparse() {
    let numbers: Vec<String> = (0..200_000).map(|n| n.to_string()).collect();
    let text = format!("[{}]", numbers.join(","));
    let mut buffer = Buffer::new(&text);
    let mut s = Syntax::new(language_by_name("json").unwrap(), &buffer.snapshot());
    let started = Instant::now();
    s.finish(s.parse_job().run());
    let parse_time = started.elapsed();
    let tail = text.len() - 1000..text.len();
    let first = bracket_depths(&s, tail.clone());
    assert_eq!(first.last().map(|b| b.depth), Some(0));
    // Best of several: a load spike hits one sample, not the minimum.
    let mut call_time = Duration::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        assert_eq!(bracket_depths(&s, tail.clone()), first);
        call_time = call_time.min(started.elapsed());
    }
    assert!(
        call_time * 20 < parse_time,
        "a call took {call_time:?}, parsing {parse_time:?}"
    );

    // Wrapped in another array, the old pair is one deeper.
    buffer.edit([(0..0, "["), (text.len()..text.len(), "]")], Origin::User);
    s.edit(&buffer.snapshot());
    s.finish(s.parse_job().run());
    let len = text.len() + 2;
    assert_eq!(
        depths(&s, &buffer.snapshot().text().to_string(), len - 3..len),
        [(len - 2, 1), (len - 1, 0)]
    );
}
