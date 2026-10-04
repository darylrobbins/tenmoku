use std::path::Path;

use fe_buffer::{Buffer, Origin};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};
use tree_sitter::{InputEdit, Point};

use super::input_edits;
use crate::{Syntax, language_for_path};

fn edit(
    bytes: (usize, usize, usize),
    start: (usize, usize),
    old_end: (usize, usize),
    new_end: (usize, usize),
) -> InputEdit {
    InputEdit {
        start_byte: bytes.0,
        old_end_byte: bytes.1,
        new_end_byte: bytes.2,
        start_position: Point::new(start.0, start.1),
        old_end_position: Point::new(old_end.0, old_end.1),
        new_end_position: Point::new(new_end.0, new_end.1),
    }
}

#[test]
fn one_batch_gives_one_edit_per_cursor() {
    let mut buffer = Buffer::new("aa\nbb\ncc\ndd\nee\n");
    let before = buffer.snapshot();

    // Spans rows and adds some; then a same-row delete, which starts two
    // bytes later once the first edit is applied.
    buffer.edit([(1..4, "X\nY\nZ"), (7..8, "")], Origin::User);
    assert_eq!(buffer.text().to_string(), "aX\nY\nZb\nc\ndd\nee\n");
    let edits = input_edits(before.text(), buffer.current(), before.version());
    assert_eq!(
        edits,
        [
            edit((1, 4, 6), (0, 1), (1, 1), (2, 1)),
            edit((9, 10, 9), (3, 1), (3, 2), (3, 1)),
        ]
    );

    // A pure insertion, and a replacement across a row boundary.
    let before = buffer.snapshot();
    buffer.edit([(0..0, "\n"), (12..14, "QQQ")], Origin::User);
    let edits = input_edits(before.text(), buffer.current(), before.version());
    assert_eq!(
        edits,
        [
            edit((0, 0, 1), (0, 0), (0, 0), (1, 0)),
            edit((13, 15, 16), (5, 2), (6, 1), (5, 5)),
        ]
    );

    // Two batches merge into one span.
    let before = buffer.snapshot();
    buffer.edit([(0..1, "")], Origin::User);
    let after_first = buffer.len();
    buffer.edit([(after_first - 1..after_first, "")], Origin::User);
    let edits = input_edits(before.text(), buffer.current(), before.version());
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].start_byte, 0);
    assert_eq!(edits[0].old_end_byte, before.len());
    assert_eq!(edits[0].new_end_byte, buffer.len());
}

/// Like `incremental_reparse_matches_a_full_parse`, but every step is one
/// batch of two to four edits from the first row to the last, synced alone.
#[test]
fn multi_cursor_reparse_matches_a_full_parse() {
    let text =
        "fn main() {\n    let x = \"s\";\n    /* c */ call(x, 1);\n    if x { y }\n}\n".repeat(6);
    let pieces = [
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
    ];
    let language = language_for_path(Path::new("x.rs")).unwrap();
    let mut rng = StdRng::seed_from_u64(23);
    let mut buffer = Buffer::new(&text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    for step in 0..300 {
        let rows = buffer.text().row_count();
        let count = rng.random_range(2..5);
        // Sorted rows, the first and last spread across the file.
        let mut rows_hit: Vec<u32> = (0..count)
            .map(|k| match k {
                0 => 0,
                k if k == count - 1 => rows - 1,
                _ => rng.random_range(0..rows),
            })
            .collect();
        rows_hit.sort();
        let mut edits = Vec::new();
        let mut floor = 0;
        for row in rows_hit {
            let line = buffer.text().row_start(row);
            let start = (line + rng.random_range(0..8)).clamp(floor, buffer.len());
            let end = (start + rng.random_range(0..4)).min(buffer.len());
            edits.push((start..end, pieces[rng.random_range(0..pieces.len())]));
            floor = end;
        }
        buffer.edit(edits, Origin::User);
        syntax.edit(buffer.current());
        syntax.finish(syntax.parse_job().run());
        let mut fresh = Syntax::new(language, buffer.current());
        fresh.finish(fresh.parse_job().run());
        assert_eq!(
            syntax.tree().unwrap().root_node().to_sexp(),
            fresh.tree().unwrap().root_node().to_sexp(),
            "after step {step}: {:?}",
            buffer.text().to_string()
        );
        let len = buffer.len();
        assert_eq!(
            syntax.highlights(0..len),
            fresh.highlights(0..len),
            "step {step}"
        );
    }
}
