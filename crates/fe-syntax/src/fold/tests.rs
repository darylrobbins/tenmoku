use std::{collections::BTreeSet, ops::Range, path::Path};

use fe_buffer::Buffer;

use super::*;
use crate::{Language, language_by_name, language_for_path, language_names};

fn parsed(language: &'static Language, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
}

/// `fold_ranges` as `(first, last)` rows.
fn folds(syntax: &Syntax, rows: Range<u32>) -> Vec<(u32, u32)> {
    fold_ranges(syntax, rows)
        .into_iter()
        .map(|range| (*range.start(), *range.end()))
        .collect()
}

/// A file name, its rows, and the folds `fold_ranges` finds in them as
/// `(first, last)` rows.
type Case = (&'static str, &'static [&'static str], &'static [(u32, u32)]);

/// A sample in each language. Brace bodies end before their closing row,
/// comments before a row holding only their `*/`,
/// as do Lua's and Ruby's before `end` and HTML's and XML's before their end
/// tags. Python, Lua and Scala bodies fold from their header's row.
const CASES: &[Case] = &[
    (
        "a.rs",
        &[
            "fn main() {",
            "    let v = vec![",
            "        1,",
            "    ];",
            "    if v.is_empty() {",
            "  return;",
            "        } else {",
            "        f(a,",
            "          b);",
            "    }",
            "}",
            "/* a",
            "   b */",
            "struct S {",
            "    a: u32,",
            "}",
        ],
        &[(0, 9), (1, 2), (4, 5), (6, 8), (7, 8), (11, 12), (13, 14)],
    ),
    (
        "a.py",
        &[
            "def f(x):",
            "    if x:",
            "        return 1",
            "    else:",
            "        return 2",
            "",
            "d = {",
            "    \"a\": 1,",
            "}",
            "class C:",
            "    pass",
        ],
        &[(0, 4), (1, 2), (3, 4), (6, 7), (9, 10)],
    ),
    (
        "a.js",
        &[
            "function f(a) {",
            "  return {",
            "    b: [",
            "      1,",
            "    ],",
            "  };",
            "}",
            "/**",
            " * doc",
            " */",
            "class C {",
            "  m() {",
            "    x();",
            "  }",
            "}",
        ],
        &[(0, 5), (1, 4), (2, 3), (7, 8), (10, 13), (11, 12)],
    ),
    (
        "a.ts",
        &[
            "function f(a) {",
            "  return {",
            "    b: [",
            "      1,",
            "    ],",
            "  };",
            "}",
            "/**",
            " * doc",
            " */",
            "class C {",
            "  m() {",
            "    x();",
            "  }",
            "}",
            "interface I {",
            "  a: number;",
            "}",
            "enum E {",
            "  A,",
            "}",
        ],
        &[
            (0, 5),
            (1, 4),
            (2, 3),
            (7, 8),
            (10, 13),
            (11, 12),
            (15, 16),
            (18, 19),
        ],
    ),
    (
        "a.tsx",
        &["const x = (", "  <div>", "    <p>hi</p>", "  </div>", ");"],
        &[(1, 2)],
    ),
    (
        "a.json",
        &[
            "{",
            "  \"a\": [",
            "    1,",
            "    2",
            "  ],",
            "  \"b\": {}",
            "}",
        ],
        &[(0, 5), (1, 3)],
    ),
    (
        "a.toml",
        &[
            "[package]",
            "name = \"x\"",
            "edition = \"2024\"",
            "",
            "[deps]",
            "a = [",
            "  1,",
            "]",
        ],
        &[(0, 2), (4, 7), (5, 6)],
    ),
    (
        "a.go",
        &[
            "package main",
            "",
            "import (",
            "\t\"fmt\"",
            ")",
            "",
            "type S struct {",
            "\ta int",
            "}",
            "",
            "func main() {",
            "\tif x {",
            "\t\tfmt.Println(",
            "\t\t\t\"a\",",
            "\t\t)",
            "\t}",
            "}",
        ],
        &[(2, 3), (6, 7), (10, 15), (11, 14), (12, 13)],
    ),
    (
        "a.c",
        &[
            "struct s {",
            "    int a;",
            "};",
            "int main(void)",
            "{",
            "    if (x) {",
            "        return 1;",
            "    }",
            "    /* multi",
            "       line */",
            "    return 0;",
            "}",
        ],
        &[(0, 1), (4, 10), (5, 6), (8, 9)],
    ),
    (
        "a.cpp",
        &[
            "struct s {",
            "    int a;",
            "};",
            "int main(void)",
            "{",
            "    if (x) {",
            "        return 1;",
            "    }",
            "    /* multi",
            "       line */",
            "    return 0;",
            "}",
            "namespace n {",
            "class C {",
            "  void f() {",
            "  }",
            "};",
            "}",
        ],
        &[(0, 1), (4, 10), (5, 6), (8, 9), (12, 16), (13, 15)],
    ),
    (
        "a.sh",
        &[
            "f() {",
            "  echo a",
            "}",
            "if [ x ]; then",
            "  echo b",
            "else",
            "  echo c",
            "fi",
            "for i in 1 2; do",
            "  echo $i",
            "done",
            "case $x in",
            "  a)",
            "    echo a",
            "    ;;",
            "esac",
        ],
        &[(0, 1), (3, 6), (8, 9), (11, 14), (12, 13)],
    ),
    (
        "a.html",
        &[
            "<html>",
            "  <body>",
            "    <p>",
            "      hi",
            "    </p>",
            "  </body>",
            "</html>",
            "<!-- a",
            "b -->",
        ],
        &[(0, 5), (1, 4), (2, 3), (7, 8)],
    ),
    (
        "a.css",
        &[
            "a {",
            "  color: red;",
            "}",
            "@media screen {",
            "  b {",
            "    color: blue;",
            "  }",
            "}",
            "/* c",
            "   d */",
        ],
        &[(0, 1), (3, 6), (4, 5), (8, 9)],
    ),
    ("a.gitignore", &["# c", "*.log", "build/"], &[]),
    ("COMMIT_EDITMSG", &["subject", "", "body", "# c"], &[]),
    (
        "git-rebase-todo",
        &["pick abc1234 one", "# c", "drop def5678 two"],
        &[],
    ),
    (
        ".gitconfig",
        &[
            "# c",
            "[user]",
            "    name = a",
            "    email = b",
            "",
            "[core]",
            "    bare = false",
        ],
        &[(1, 3), (5, 6)],
    ),
    (
        ".gitattributes",
        &["# c", "*.png binary", "*.sh eol=lf"],
        &[],
    ),
    ("x.ini", &["; c", "[a]", "b = 1", "c = 2"], &[]),
    (".env", &["# c", "A=1", "B=\"2\""], &[]),
    (
        "justfile",
        &[
            "# c",
            "x := \"1\"",
            "",
            "build target:",
            "    echo {{target}}",
            "    cargo build",
            "",
            "one:",
            "    echo one",
        ],
        &[(3, 5), (7, 8)],
    ),
    (
        "x.diff",
        &[
            "diff --git a/f b/f",
            "index 83db48f..bf2a1c0 100644",
            "--- a/f",
            "+++ b/f",
            "@@ -1,2 +1,2 @@",
            " a",
            "-b",
            "+c",
            "@@ -9,2 +9,2 @@",
            " d",
            "-e",
            "+f",
        ],
        &[(0, 11), (4, 7), (8, 11)],
    ),
    ("x.csv", &["a,\"b", "c\",d", "e,f"], &[]),
    ("x.tsv", &["a\t\"b", "c\"\td", "e\tf"], &[]),
    (
        "x.json5",
        &["{", "  a: [", "    1,", "  ],", "  // c", "  b: 2,", "}"],
        &[(0, 5), (1, 2)],
    ),
    (
        "a.yaml",
        &[
            "a:", "  b: 1", "  c:", "    - 1", "    - 2", "d: |", "  text", "  more", "e: 3",
        ],
        &[(0, 4), (2, 4), (5, 7)],
    ),
    (
        "a.md",
        &[
            "# Title",
            "",
            "Text.",
            "",
            "## Sub",
            "",
            "More.",
            "",
            "```rust",
            "fn x() {}",
            "```",
            "",
            "# Two",
            "",
            "End.",
        ],
        &[(0, 10), (4, 10), (8, 9), (12, 14)],
    ),
    (
        "a.java",
        &[
            "class A {",
            "    void f() {",
            "        if (x) {",
            "            y();",
            "        }",
            "    }",
            "    /* c",
            "       d */",
            "}",
        ],
        &[(0, 7), (1, 4), (2, 3), (6, 7)],
    ),
    (
        "a.rb",
        &[
            "class A",
            "  def f(x)",
            "    if x",
            "      1",
            "    else",
            "      2",
            "    end",
            "  end",
            "end",
            "h = {",
            "  a: 1,",
            "}",
            "[1, 2].each do |x|",
            "  p x",
            "end",
        ],
        &[(0, 7), (1, 6), (2, 3), (2, 5), (4, 5), (9, 10), (12, 13)],
    ),
    (
        "a.lua",
        &[
            "function f(x)",
            "  if x then",
            "    return 1",
            "  end",
            "end",
            "local t = {",
            "  a = 1,",
            "}",
            "--[[ a",
            "b ]]",
        ],
        &[(0, 3), (1, 2), (5, 6), (8, 9)],
    ),
    (
        "a.php",
        &[
            "<?php",
            "class A {",
            "    function f() {",
            "        return [",
            "            1,",
            "        ];",
            "    }",
            "}",
        ],
        &[(1, 6), (2, 5), (3, 4)],
    ),
    (
        "a.cs",
        &[
            "namespace N",
            "{",
            "    class A",
            "    {",
            "        void F()",
            "        {",
            "            if (x) {",
            "                y();",
            "            }",
            "        }",
            "    }",
            "}",
        ],
        &[(1, 10), (3, 9), (5, 8), (6, 7)],
    ),
    (
        "a.scala",
        &[
            "object A {",
            "  def f(x: Int) = {",
            "    x match {",
            "      case 1 => 2",
            "    }",
            "  }",
            "}",
            "object B:",
            "  def g =",
            "    1",
            "    2",
        ],
        &[(0, 5), (1, 4), (2, 3), (7, 10), (8, 10)],
    ),
    (
        "a.xml",
        &[
            "<a>", "  <b>", "    x", "  </b>", "  <!-- c", "  d -->", "</a>",
        ],
        &[(0, 5), (1, 2), (4, 5)],
    ),
    (
        "a.swift",
        &[
            "class A {",
            "    func f() {",
            "        if x {",
            "            y()",
            "        }",
            "    }",
            "}",
        ],
        &[(0, 5), (1, 4), (2, 3)],
    ),
    (
        "a.kt",
        &[
            "class A {",
            "    fun f() {",
            "        if (x) {",
            "            y()",
            "        }",
            "        when (x) {",
            "            1 -> 2",
            "        }",
            "    }",
            "}",
        ],
        &[(0, 8), (1, 7), (2, 3), (5, 6)],
    ),
    (
        "a.sql",
        &[
            "CREATE TABLE a (",
            "  id INT,",
            "  name TEXT",
            ");",
            "SELECT a,",
            "  b",
            "FROM t;",
        ],
        &[(0, 2), (0, 3), (4, 6)],
    ),
    (
        "Dockerfile",
        &[
            "FROM alpine",
            "RUN apk add \\",
            "    curl \\",
            "    git",
            "RUN <<EOT",
            "echo a",
            "echo b",
            "EOT",
        ],
        &[(1, 3), (4, 7)],
    ),
    (
        "Makefile",
        &[
            "all: a",
            "\techo a",
            "\techo b",
            "",
            "ifeq ($(X),1)",
            "Y = 2",
            "endif",
        ],
        &[(0, 6), (4, 5)],
    ),
];

#[test]
fn candidates_per_language() {
    let mut tested = BTreeSet::new();
    for &(file, rows, expected) in CASES {
        // Languages left out of this build aren't tested.
        let Some(language) = language_for_path(Path::new(file)) else {
            continue;
        };
        tested.insert(language.name());
        let syntax = parsed(language, &rows.join("\n"));
        assert_eq!(folds(&syntax, 0..rows.len() as u32), expected, "{file}");
    }
    let languages: BTreeSet<_> = language_names().collect();
    assert_eq!(tested, languages, "a sample per language");
}

#[test]
fn fold_range_at_prefers_the_header_then_innermost() {
    let Some(rust) = language_by_name("rust") else {
        return;
    };
    let text = [
        "fn main() {",
        "    foo(1, |x| {",
        "        x",
        "    });",
        "    let a = [",
        "        1,",
        "    ];",
        "}",
    ]
    .join("\n");
    let syntax = parsed(rust, &text);
    let at = |row| fold_range_at(&syntax, row).map(|range| (*range.start(), *range.end()));
    // Row 1 heads the call's arguments, which end at `)`, and the closure's
    // body inside them, which ends before `}`.
    assert_eq!(folds(&syntax, 1..2), [(1, 2), (1, 3)]);
    assert_eq!(
        (0..8).map(at).collect::<Vec<_>>(),
        [
            Some((0, 6)),
            Some((1, 2)),
            Some((1, 2)),
            Some((1, 3)),
            Some((4, 5)),
            Some((4, 5)),
            Some((0, 6)),
            None,
        ]
    );
}

#[test]
fn ranges_start_in_the_rows_asked_for() {
    let Some(python) = language_by_name("python") else {
        return;
    };
    let text = "def f(x):\n    if x:\n        return 1\n    return 2\n";
    let syntax = parsed(python, text);
    assert_eq!(folds(&syntax, 0..5), [(0, 3), (1, 2)]);
    // A body below the rows still folds from a header in them.
    assert_eq!(folds(&syntax, 0..1), [(0, 3)]);
    assert_eq!(folds(&syntax, 1..2), [(1, 2)]);
    assert_eq!(folds(&syntax, 2..5), []);
    assert_eq!(fold_range_at(&syntax, 2), Some(1..=2));
    assert_eq!(fold_range_at(&syntax, 3), Some(0..=3));
}

/// Checks `fold_ranges` over every row, and `fold_range_at` the first row,
/// for `text` in the language `file` names, if it's in this build.
fn check(file: &str, text: &str, expected: &[(u32, u32)]) {
    let Some(language) = language_for_path(Path::new(file)) else {
        return;
    };
    let syntax = parsed(language, text);
    let rows = text.split('\n').count() as u32;
    assert_eq!(folds(&syntax, 0..rows), expected, "{file}: {text:?}");
    let at = fold_range_at(&syntax, 0).map(|range| (*range.start(), *range.end()));
    let first = expected.iter().find(|(start, _)| *start == 0).copied();
    assert_eq!(at, first, "{file}: {text:?} at row 0");
}

#[test]
fn unclosed_delimiters_fold_nothing() {
    check("a.md", "```\n", &[]);
    check("a.md", "~~~\n", &[]);
    check("a.rs", "/*\n", &[]);
    check("a.rs", "fn f() {\n", &[]);
    check("a.json", "[\n", &[]);
}

#[test]
fn comments_between_a_header_and_its_body() {
    for (file, text) in [
        ("a.py", "def f():\n    # comment\n    x()\n"),
        ("a.lua", "function f()\n-- comment\nx()\nend"),
    ] {
        check(file, text, &[(0, 2)]);
        let Some(language) = language_for_path(Path::new(file)) else {
            continue;
        };
        let syntax = parsed(language, text);
        assert_eq!(folds(&syntax, 0..1), [(0, 2)], "{file}: header row only");
    }
}

#[test]
fn comments_end_before_a_row_of_just_their_closer() {
    // `(file, opening, closing)`: a comment folds to the row before its
    // closing delimiter when that's alone on its row, and keeps the row when
    // it has text too.
    for (file, open, close) in [
        ("a.js", "/*", "*/"),
        ("a.ts", "/*", "*/"),
        ("a.tsx", "/*", "*/"),
        ("a.c", "/*", "*/"),
        ("a.cpp", "/*", "*/"),
        ("a.go", "/*", "*/"),
        ("a.css", "/*", "*/"),
        ("a.java", "/*", "*/"),
        ("a.php", "<?php\n/*", "*/"),
        ("a.cs", "/*", "*/"),
        ("a.kt", "/*", "*/"),
        ("a.swift", "/*", "*/"),
        ("a.sql", "/*", "*/"),
        ("a.rs", "/*", "*/"),
        ("a.html", "<!--", "-->"),
        ("a.xml", "<a>\n<!--", "-->\n</a>"),
        ("a.rb", "=begin", "=end"),
        ("a.lua", "--[[", "]]"),
    ] {
        // The comment's first row, after any lines `open` puts before it.
        let first = open.matches('\n').count() as u32;
        let alone = format!("{open}\n  a\n  b\n  {close}");
        let shared = format!("{open}\n  a\n  b {close}");
        for (text, last) in [(alone, first + 2), (shared, first + 2)] {
            let Some(language) = language_for_path(Path::new(file)) else {
                continue;
            };
            let syntax = parsed(language, &text);
            let rows = text.split('\n').count() as u32;
            assert!(
                folds(&syntax, 0..rows).contains(&(first, last)),
                "{file}: {text:?} folds {:?}",
                folds(&syntax, 0..rows)
            );
        }
    }
}

#[test]
fn no_tree_no_candidates() {
    let Some(rust) = language_by_name("rust") else {
        return;
    };
    let buffer = Buffer::new("fn f() {\n    x\n}\n");
    let syntax = Syntax::new(rust, buffer.current());
    assert_eq!(folds(&syntax, 0..3), []);
    assert_eq!(fold_range_at(&syntax, 0), None);
}

#[test]
fn every_kind_is_in_its_grammar() {
    for kinds in KINDS {
        let Some(language) = language_by_name(kinds.language) else {
            continue;
        };
        for kind in kinds.nodes.iter().chain(kinds.bodies) {
            assert_ne!(
                language.grammar.id_for_node_kind(kind, true),
                0,
                "{}: no {kind} nodes",
                kinds.language
            );
        }
    }
}
