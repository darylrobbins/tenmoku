use std::{cmp::Reverse, ops::Range, path::Path, time::Duration};

mod golden;
mod query_overrides;

use fe_buffer::{Buffer, Origin};
use proptest::{prelude::prop, prop_assert, prop_assert_eq, proptest};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::{
    highlight::{Capture, flatten},
    syntax::FAULT,
    *,
};

/// A fully parsed tree for `buffer`.
fn parsed(language: &'static Language, buffer: &Buffer) -> Syntax {
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
}

fn rust() -> &'static Language {
    language_by_name("rust").unwrap()
}

/// `(text, highlight)` for every span, for readable assertions.
fn spans(syntax: &Syntax, text: &str, range: Range<usize>) -> Vec<(String, &'static str)> {
    syntax
        .highlights(range)
        .into_iter()
        .map(|span| (text[span.range].to_string(), span.highlight.name()))
        .collect()
}

/// A file name, a snippet, and `(text, highlight)` spans its highlights must include.
type Snippet = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

#[test]
fn each_language_highlights_a_snippet() {
    let cases: &[Snippet] = &[
        (
            "main.rs",
            "fn main() { let s = \"a\\n\"; // hi\n}",
            &[
                ("fn", "keyword"),
                ("main", "function"),
                ("\"a", "string"),
                ("\\n", "escape"),
                ("// hi", "comment"),
            ],
        ),
        (
            "x.py",
            "def f(x):\n    return None  # c\n",
            &[
                ("def", "keyword"),
                ("f", "function"),
                ("None", "constant.builtin"),
                ("# c", "comment"),
            ],
        ),
        (
            "x.js",
            "const a = () => <div>{1}</div>;",
            &[("const", "keyword"), ("div", "tag"), ("1", "number")],
        ),
        (
            "x.ts",
            "interface A { b: string }",
            &[
                ("interface", "keyword"),
                ("A", "type"),
                ("string", "type.builtin"),
            ],
        ),
        (
            "x.tsx",
            "let a: number = <b/>;",
            &[("let", "keyword"), ("number", "type.builtin"), ("b", "tag")],
        ),
        (
            "x.json",
            "{\"k\": [1, true]}",
            &[
                ("\"k\"", "string.special.key"),
                ("1", "number"),
                ("true", "constant.builtin"),
            ],
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"x\" # c\n",
            &[
                ("package", "type"),
                ("name", "property"),
                ("\"x\"", "string"),
                ("# c", "comment"),
            ],
        ),
        (
            "x.go",
            "package main\nfunc f() int { return 1 }",
            &[
                ("package", "keyword"),
                ("f", "function"),
                ("int", "type"),
                ("1", "number"),
            ],
        ),
        (
            "x.c",
            "#include <stdio.h>\nint main(void) { return 0; }",
            &[
                ("#include", "keyword"),
                ("int", "type"),
                ("main", "function"),
            ],
        ),
        (
            "x.cpp",
            "namespace n { class C {}; }",
            &[
                ("namespace", "keyword"),
                ("class", "keyword"),
                ("C", "type"),
            ],
        ),
        (
            "x.sh",
            "echo \"$HOME\" # c",
            &[
                ("echo", "function"),
                ("HOME", "property"),
                ("# c", "comment"),
            ],
        ),
        (
            "x.html",
            "<p class=\"a\">hi</p>",
            &[("p", "tag"), ("class", "attribute"), ("a", "string")],
        ),
        (
            "x.css",
            "a { color: #fff; }",
            &[
                ("a", "tag"),
                ("color", "property"),
                ("fff", "string.special"),
            ],
        ),
        (
            "x.yaml",
            "key: 1 # c",
            &[("key", "property"), ("1", "number"), ("# c", "comment")],
        ),
        (
            "README.md",
            "# Title\n\n```\ncode\n```\n",
            &[("Title", "markup.heading"), ("\ncode\n", "markup.raw")],
        ),
        (
            "X.java",
            "class X { int f() { return 1; } }",
            &[
                ("class", "keyword"),
                ("int", "type.builtin"),
                ("f", "function.method"),
            ],
        ),
        (
            "Gemfile",
            "def f(x) = x\nv = f(:sym) # c",
            &[
                ("def", "keyword"),
                ("f", "function.method"),
                ("v", "variable"),
                (":sym", "string.special"),
                ("# c", "comment"),
            ],
        ),
        (
            "x.lua",
            "local function f(x) return nil end -- c",
            &[
                ("local", "keyword"),
                ("f", "function"),
                ("nil", "constant.builtin"),
                ("-- c", "comment"),
            ],
        ),
        (
            "x.php",
            "<?php function f($x) { return null; } // c",
            &[
                ("<?php", "tag"),
                ("function", "keyword"),
                ("f", "function"),
                ("x", "variable"),
                ("null", "constant.builtin"),
                ("// c", "comment"),
            ],
        ),
        (
            "x.cs",
            "class C { int F() => 1; } // c",
            &[
                ("class", "keyword"),
                ("C", "type"),
                ("int", "type.builtin"),
                ("F", "function"),
                ("// c", "comment"),
            ],
        ),
        (
            "x.scala",
            "object O { def f(x: Int) = \"s\" } // c",
            &[
                ("object", "keyword"),
                ("def", "keyword"),
                ("f", "function.method"),
                ("Int", "type"),
                ("\"s\"", "string"),
                ("// c", "comment"),
            ],
        ),
        (
            "x.svg",
            "<svg width=\"1\"><!-- c --></svg>",
            &[
                ("svg", "tag"),
                ("width", "property"),
                ("<!-- c -->", "comment"),
            ],
        ),
        (
            "x.swift",
            "func f(x: Int) -> String { return \"s\" } // c",
            &[
                ("func", "keyword"),
                ("f", "function.method"),
                ("Int", "type"),
                ("\"s\"", "string"),
                ("// c", "comment"),
            ],
        ),
        (
            "x.kt",
            "private fun f(x: Int) = null // c",
            &[
                ("fun", "keyword"),
                ("f", "function"),
                ("x", "variable.parameter"),
                ("Int", "type.builtin"),
                ("null", "constant.builtin"),
                ("// c", "comment"),
            ],
        ),
        (
            "x.sql",
            "SELECT name, 1 FROM t WHERE x = 'a'; -- c",
            &[
                ("SELECT", "keyword"),
                ("name", "property"),
                ("1", "number"),
                ("t", "type"),
                ("'a'", "string"),
                ("-- c", "comment"),
            ],
        ),
        (
            "Containerfile",
            "FROM alpine AS base\nRUN echo \"hi\" && ls -l",
            &[
                ("FROM", "keyword"),
                ("AS", "keyword"),
                ("RUN", "keyword"),
                ("echo", "function"),
                ("\"hi\"", "string"),
                ("-l", "constant"),
            ],
        ),
        (
            "GNUmakefile",
            "# c\nCC ?= cc\nall: $(SRC)\n\techo hi",
            &[
                ("CC", "constant.builtin"),
                ("?=", "operator"),
                ("# c", "comment"),
                ("SRC", "constant"),
            ],
        ),
    ];
    let mut covered = Vec::new();
    let mut missing = Vec::new();
    for &(file, text, expected) in cases {
        let language = language_for_path(Path::new(file)).unwrap_or_else(|| panic!("{file}"));
        covered.push(language.name());
        let buffer = Buffer::new(text);
        let mut syntax = Syntax::new(language, buffer.current());
        assert!(syntax.parse_within(Duration::from_secs(10)));
        let found = spans(&syntax, text, 0..text.len());
        for &(token, highlight) in expected {
            if !found.contains(&(token.to_string(), highlight)) {
                missing.push(format!("{file}: {token:?} as {highlight}; got {found:?}"));
            }
        }
    }
    assert!(missing.is_empty(), "missing:\n{}", missing.join("\n"));
    let mut all: Vec<_> = language_names().collect();
    all.sort_unstable();
    covered.sort_unstable();
    assert_eq!(covered, all, "every language has a case");
}

/// Random edits, made in batches and caught up with in groups, sometimes
/// while a background parse is in flight, always leave the same tree a parse
/// from scratch would build.
#[test]
fn incremental_reparse_matches_a_full_parse() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "x.rs",
            "fn main() {\n    let x = \"s\";\n    /* c */ call(x, 1);\n}\n",
            &[
                "fn f() {}",
                "{",
                "}",
                "\"",
                "/*",
                "*/",
                "\n",
                "x",
                " ",
                "// c\n",
                "",
            ],
        ),
        (
            "x.py",
            "def f(x):\n    if x:\n        return 1\n    return 2\n",
            &[
                "\n",
                "    ",
                "if y:",
                "pass",
                "def g():\n    ",
                ":",
                "\"",
                "#",
                "x",
                "",
            ],
        ),
    ];
    let mut rng = StdRng::seed_from_u64(7);
    for &(file, text, pieces) in cases {
        let language = language_for_path(Path::new(file)).unwrap();
        let mut buffer = Buffer::new(text);
        let mut syntax = Syntax::new(language, buffer.current());
        let mut in_flight = None;
        for step in 0..300 {
            for _ in 0..rng.random_range(1..4) {
                // One batch of one to three edits.
                let edits: Vec<_> = (0..rng.random_range(1..4))
                    .map(|_| {
                        let start = rng.random_range(0..=buffer.len());
                        let end = (start + rng.random_range(0..6)).min(buffer.len());
                        (start..end, pieces[rng.random_range(0..pieces.len())])
                    })
                    .collect();
                buffer.edit(edits, Origin::User);
            }
            syntax.edit(buffer.current());
            match rng.random_range(0..3) {
                0 => in_flight = Some(syntax.parse_job()),
                1 => {
                    if let Some(job) = in_flight.take() {
                        syntax.finish(job.run());
                    }
                }
                _ => {}
            }
            if step % 5 == 0 {
                syntax.finish(syntax.parse_job().run());
                assert!(!syntax.needs_parse());
                let fresh = parsed(language, &buffer);
                assert_eq!(
                    syntax.tree().unwrap().root_node().to_sexp(),
                    fresh.tree().unwrap().root_node().to_sexp(),
                    "{file} after step {step}: {:?}",
                    buffer.text().to_string()
                );
                let len = buffer.len();
                assert_eq!(syntax.highlights(0..len), fresh.highlights(0..len));
            }
        }
    }
}

#[test]
fn highlights_only_the_requested_range() {
    let item = "/* a\n   block */\nfn f(s: &str) -> u8 { 1 } // end\n";
    let text = item.repeat(200);
    let syntax = parsed(rust(), &Buffer::new(&text));
    let all = syntax.highlights(0..text.len());

    // From inside a block comment to inside a function signature.
    let start = item.len() * 120 + 8;
    let range = start..start + item.len() * 3 + 10;
    let expected: Vec<HighlightSpan> = all
        .iter()
        .filter(|span| span.range.end > range.start && span.range.start < range.end)
        .map(|span| HighlightSpan {
            range: span.range.start.max(range.start)..span.range.end.min(range.end),
            highlight: span.highlight,
        })
        .collect();
    let got = syntax.highlights(range.clone());
    assert_eq!(got, expected);
    assert_eq!(got[0].highlight.name(), "comment");
    assert_eq!(got[0].range.start, range.start);
}

#[test]
fn an_edited_tree_highlights_before_it_is_reparsed() {
    let mut buffer = Buffer::new("fn f() {}\n");
    let mut syntax = parsed(rust(), &buffer);
    buffer.edit([(0..0, "\n\n")], Origin::User);
    syntax.edit(buffer.current());
    assert!(syntax.needs_parse());
    let text = buffer.text().to_string();
    assert_eq!(
        spans(&syntax, &text, 0..text.len())[..2],
        [("fn".into(), "keyword"), ("f".into(), "function")]
    );
    assert_eq!(syntax.highlights(0..text.len())[0].range, 2..4);
}

/// `@none` clears the highlight around it: Scala's interpolated code isn't
/// coloured as the string it's in, whether `$x` or `${…}`.
#[test]
fn none_captures_clear_the_enclosing_highlight() {
    let text = "val m = s\"a $x b ${y.size} c\"\n";
    let syntax = parsed(language_by_name("scala").unwrap(), &Buffer::new(text));
    let found = spans(&syntax, text, 0..text.len());
    for expected in [
        ("\"a ", "string"),
        ("$", "punctuation.special"),
        (" b ", "string"),
        ("{", "punctuation.bracket"),
        ("size", "property"),
        (" c\"", "string"),
    ] {
        assert!(
            found.contains(&(expected.0.to_string(), expected.1)),
            "{expected:?} in {found:?}"
        );
    }
    // `x` and `y` are plain.
    let all = syntax.highlights(0..text.len());
    for before in ["$x", "{y"] {
        let at = text.find(before).unwrap() + 1;
        assert!(
            !all.iter().any(|span| span.range.contains(&at)),
            "{before}: {found:?}"
        );
    }
}

#[test]
fn capture_names_map_to_the_most_specific_highlight() {
    let name = |capture| Highlight::from_capture(capture).map(Highlight::name);
    assert_eq!(name("function.method.call"), Some("function.method"));
    assert_eq!(name("text.title"), Some("markup.heading"));
    assert_eq!(name("boolean"), Some("constant.builtin"));
    assert_eq!(name("spell"), None);

    let theme = resolve_theme(&[("function", 1), ("function.method", 2), ("type", 3)]);
    let style = |name| theme[Highlight::from_capture(name).unwrap().index()];
    assert_eq!(style("function.builtin"), Some(1));
    assert_eq!(style("function.method"), Some(2));
    assert_eq!(style("type.builtin"), Some(3));
    assert_eq!(style("keyword"), None);
}

/// Highlighting a node with many children a child at a time gives the same
/// spans as one query over the whole tree.
#[test]
fn flat_nodes_highlight_as_one_query_would() {
    let cases: &[(&str, &str, &str, &str)] = &[
        ("rust", "", "fn f() -> u8 { 1 } // c\n", ""),
        ("python", "", "def f(x): return None  # c\n", ""),
        ("javascript", "", "const a = () => <div>{1}</div>;\n", ""),
        ("typescript", "", "let a: number = 1;\n", ""),
        ("tsx", "", "let a: number = <b/>;\n", ""),
        ("json", "[\n", "  {\"k\": [1, true]},\n", "  null\n]\n"),
        ("json", "{\"items\": [\n", "  \"a\\n\",\n", "  0\n]}\n"),
        ("toml", "", "[t]\nname = \"x\" # c\n", ""),
        ("go", "package main\n", "func f() int { return 1 }\n", ""),
        ("c", "", "int f(void) { return 0; }\n", ""),
        ("cpp", "", "namespace n { class C {}; }\n", ""),
        ("bash", "", "echo \"$HOME\" # c\n", ""),
        ("html", "", "<p class=\"a\">hi</p>\n", ""),
        ("css", "", "a { color: red; }\n", ""),
        ("yaml", "", "- item: 1 # c\n", ""),
        (
            "markdown",
            "# Title\n\n",
            "Some text.\n\n```rust\nfn f() {}\n```\n\n",
            "",
        ),
        ("java", "", "class A { int f() { return 1; } }\n", ""),
    ];
    for &(name, header, item, footer) in cases {
        let text = format!("{header}{}{footer}", item.repeat(1500));
        let buffer = Buffer::new(&text);
        let syntax = parsed(language_by_name(name).unwrap(), &buffer);
        let tree = syntax.tree().unwrap();
        let rows = buffer.text().row_count();
        let row = |row: u32| buffer.text().row_start(row);
        let mid = row(rows / 2);
        assert!(
            !crate::syntax::flat_paths(tree.root_node(), &(mid..mid + 1)).is_empty(),
            "{name}: no node has many children"
        );
        for range in [
            0..row(60),
            mid..row(rows / 2 + 60),
            mid + 3..mid + 500,
            row(rows - 60)..text.len(),
            0..text.len(),
        ] {
            assert_eq!(
                syntax.query(tree, range.clone(), true, Duration::MAX),
                syntax.query(tree, range.clone(), false, Duration::MAX),
                "{name} {range:?}"
            );
        }
    }
}

/// Highlights are cached per range until an edit or a reparse.
#[test]
fn cached_highlights_follow_edits_and_reparses() {
    let mut buffer = Buffer::new("let x = 1;\n");
    let mut syntax = parsed(rust(), &buffer);
    let first = |syntax: &Syntax| {
        let span = syntax.highlights(0..10)[0].clone();
        (span.range, span.highlight.name())
    };
    assert_eq!(first(&syntax), (0..3, "keyword"));
    assert_eq!(first(&syntax), (0..3, "keyword"));
    buffer.edit([(0..0, "// ")], Origin::User);
    syntax.edit(buffer.current());
    assert_eq!(first(&syntax), (3..6, "keyword"));
    syntax.finish(syntax.parse_job().run());
    assert_eq!(first(&syntax), (0..10, "comment"));
}

/// A parse that panics, on a background thread or within a frame's budget,
/// leaves the buffer as plain text that can still be edited.
#[test]
fn a_panicking_parse_leaves_plain_text() {
    for background in [true, false] {
        let mut buffer = Buffer::new("fn f() {}\n");
        let mut syntax = Syntax::new(rust(), buffer.current());
        if background {
            let job = syntax.parse_job();
            let parsed = std::thread::spawn(move || {
                FAULT.set(Some("read"));
                job.run()
            });
            syntax.finish(parsed.join().expect("the panic is caught"));
        } else {
            FAULT.set(Some("read"));
            assert!(syntax.parse_within(Duration::from_secs(10)));
        }
        assert!(syntax.failed() && !syntax.needs_parse());
        assert_eq!(syntax.highlights(0..10), []);

        buffer.edit([(0..0, "// ")], Origin::User);
        syntax.edit(buffer.current());
        assert!(syntax.tree().is_none() && !syntax.needs_parse());
        assert!(syntax.parse_within(Duration::from_secs(10)));
        assert_eq!(syntax.highlights(0..13), []);
    }
}

/// A query that panics leaves the buffer as plain text, and doesn't try again.
#[test]
fn a_panicking_query_leaves_plain_text() {
    let syntax = parsed(rust(), &Buffer::new("fn f() {}\n"));
    FAULT.set(Some("query"));
    assert_eq!(syntax.highlights(0..10), []);
    assert!(syntax.failed());
    assert_eq!(syntax.highlights(0..10), []);
}

/// A query stops at its time budget, with the highlights found so far.
#[test]
fn queries_stop_at_their_budget() {
    let text = "fn f(x: u8) -> u8 { x + 1 } // c\n".repeat(2000);
    let syntax = parsed(rust(), &Buffer::new(&text));
    let tree = syntax.tree().unwrap();
    let query = |budget| syntax.query(tree, 0..text.len(), false, budget).len();
    let (all, cut) = (query(Duration::MAX), query(Duration::ZERO));
    assert!(cut < all / 10, "{cut} of {all} spans");
}

/// `(start, len, highlight, precedence)`, for captures from proptest.
/// Highlight 0 is `@none`, and `n` the `n - 1`th highlight.
type CaptureSpec = (usize, usize, usize, usize);

fn captures(specs: &[CaptureSpec]) -> Vec<Capture> {
    specs
        .iter()
        .map(|&(start, len, highlight, precedence)| Capture {
            range: start..start + len,
            highlight: highlight
                .checked_sub(1)
                .map(|ix| Highlight::all().nth(ix).unwrap()),
            precedence,
        })
        .collect()
}

/// [`flatten`]'s rule applied to each byte of `clip` on its own: of the
/// captures covering it, the one starting last wins, then the shortest, then
/// the highest precedence, then the earliest. If it's `@none`, the byte has
/// no highlight.
fn paint(captures: &[Capture], clip: Range<usize>) -> Vec<Option<Highlight>> {
    clip.map(|byte| {
        captures
            .iter()
            .enumerate()
            .filter(|(_, capture)| capture.range.contains(&byte))
            .max_by_key(|&(ix, capture)| {
                (
                    capture.range.start,
                    Reverse(capture.range.end),
                    capture.precedence,
                    Reverse(ix),
                )
            })
            .and_then(|(_, capture)| capture.highlight)
    })
    .collect()
}

proptest! {
    /// Random captures, nested and partly overlapping, flatten to the spans
    /// painting each byte separately gives.
    #[test]
    fn flatten_matches_painting_each_byte(
        specs in prop::collection::vec((0usize..30, 0usize..15, 0usize..5, 0usize..3), 0..12),
        clip_start in 0usize..35,
        clip_len in 0usize..30,
    ) {
        let clip = clip_start..clip_start + clip_len;
        let spans = flatten(captures(&specs), clip.clone());
        let mut painted = vec![None; clip.len()];
        let mut last: Option<&HighlightSpan> = None;
        for span in &spans {
            prop_assert!(!span.range.is_empty(), "{spans:?}");
            prop_assert!(clip.start <= span.range.start && span.range.end <= clip.end);
            if let Some(last) = last {
                prop_assert!(last.range.end <= span.range.start, "{spans:?}");
                prop_assert!(
                    last.range.end < span.range.start || last.highlight != span.highlight,
                    "touching spans with one highlight aren't merged: {spans:?}"
                );
            }
            for byte in span.range.clone() {
                painted[byte - clip.start] = Some(span.highlight);
            }
            last = Some(span);
        }
        prop_assert_eq!(painted, paint(&captures(&specs), clip));
    }
}
