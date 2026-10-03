use std::{ops::Range, path::Path, time::Duration};

use fe_buffer::{Buffer, Origin};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use crate::*;

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
                ("name", "type"),
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
