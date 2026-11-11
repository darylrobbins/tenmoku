//! [`fold_candidate_closing`]: the row a candidate's node closes on, where
//! the gutter's up-chevron goes, whether the closer is the node's own or,
//! for a body, its parent's; and none past a body with no closer.

use std::path::Path;

use fe_buffer::Buffer;

use super::*;
use crate::language_for_path;

/// `text` parsed in the language `file` names, if it's in this build.
fn parsed(file: &str, text: &str) -> Option<Syntax> {
    let language = language_for_path(Path::new(file))?;
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    Some(syntax)
}

/// The candidate headed by `row` and its closing row, as `(first, last,
/// closing)`.
fn closing(syntax: &Syntax, row: u32) -> Option<(u32, u32, u32)> {
    let (range, closing) = fold_candidate_closing(syntax, row)?;
    Some((*range.start(), *range.end(), closing))
}

#[test]
fn closers_of_the_node_itself() {
    let Some(syntax) = parsed(
        "a.rs",
        "/*\n  a\n*/\nfn f() {\n    call(\n        a,\n        b);\n}\n",
    ) else {
        return;
    };
    assert_eq!(closing(&syntax, 0), Some((0, 1, 2)), "the comment's */");
    assert_eq!(closing(&syntax, 3), Some((3, 6, 7)), "the block's }}");
    // `)` doesn't start its row: the call closes on its last row.
    assert_eq!(closing(&syntax, 4), Some((4, 6, 6)), "the call");
    assert_eq!(closing(&syntax, 1), None, "no candidate starts on row 1");
}

#[test]
fn a_body_closes_on_its_parent_s_closer() {
    let cases = [
        ("a.lua", "function f()\n  x()\nend\n", 0, (0, 1, 2)),
        (
            "a.swift",
            "func f() {\n    if a {\n        b()\n        c()\n    }\n}\n",
            1,
            (1, 3, 4),
        ),
        (
            "a.php",
            "<?php\nif ($a):\n    x();\n    y();\nendif;\n",
            1,
            (1, 3, 4),
        ),
        (
            "a.php",
            "<?php\nforeach ($a as $b):\n    x();\n    y();\nendforeach;\n",
            1,
            (1, 3, 4),
        ),
    ];
    for (file, text, row, want) in cases {
        let Some(syntax) = parsed(file, text) else {
            continue;
        };
        assert_eq!(closing(&syntax, row), Some(want), "{file}: {text:?}");
    }
}

#[test]
fn a_body_with_no_closer_closes_on_its_last_row() {
    for (file, text) in [
        ("a.py", "def f():\n    x()\n    y()\nz = 1\n"),
        // `end` on the body's last row doesn't start it.
        ("a.lua", "function f()\n  x()\n  y() end\n"),
    ] {
        let Some(syntax) = parsed(file, text) else {
            continue;
        };
        assert_eq!(closing(&syntax, 0), Some((0, 2, 2)), "{file}");
    }
}
