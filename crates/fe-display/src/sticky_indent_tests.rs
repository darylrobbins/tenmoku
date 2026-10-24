//! [`indent_scopes`]: the indented blocks enclosing some rows, for text
//! without a tree, as fe-editor's indented-block folds find them, and found
//! without scanning far below the rows.

#![allow(clippy::single_range_in_vec_init)]

use std::ops::{Range, RangeInclusive};

use super::*;

/// Every indented block of `text`, by fe-editor's rules (its
/// `indented_blocks`), sorted by header.
fn all_blocks(text: &Text, tab_size: usize) -> Vec<RangeInclusive<u32>> {
    let mut blocks = Vec::new();
    let mut open: Vec<(u32, usize)> = Vec::new();
    let mut last_text_row = 0;
    let mut close = |open: &mut Vec<(u32, usize)>, level: usize, last_text_row: u32| {
        while let Some(&(header, _)) = open.last().filter(|(_, indent)| *indent >= level) {
            open.pop();
            if last_text_row > header {
                blocks.push(header..=last_text_row);
            }
        }
    };
    let rows = 0..text.row_count();
    for (row, indent) in rows.clone().zip(indents(text, rows, tab_size)) {
        let Some(level) = indent else {
            continue;
        };
        close(&mut open, level, last_text_row);
        open.push((row, level));
        last_text_row = row;
    }
    close(&mut open, 0, last_text_row);
    blocks.sort_by_key(|block| *block.start());
    blocks
}

/// What [`indent_scopes`] should give for `rows`, from every block: those
/// overlapping the rows, once what it can't see below the last row it looks
/// at, the one past them, is taken as open. A block still open on the last
/// text row it sees ends at `rows.end`, and one headed by that row isn't
/// known to be a block at all.
fn expected(text: &Text, rows: Range<u32>, tab_size: usize) -> Vec<RangeInclusive<u32>> {
    let bound = rows.end.min(text.row_count() - 1);
    let at_end = bound == text.row_count() - 1;
    let indents = indents(text, 0..bound + 1, tab_size);
    let last_text_row = (0..=bound)
        .rev()
        .find(|&row| indents[row as usize].is_some());
    (all_blocks(text, tab_size).into_iter())
        .filter_map(|block| match last_text_row {
            Some(last) if !at_end && *block.end() >= last => {
                (*block.start() < last).then(|| *block.start()..=rows.end)
            }
            _ => Some(block),
        })
        .filter(|block| *block.start() < rows.end && rows.start <= *block.end())
        .collect()
}

const TEXT: &str = "\
a:
    b:
        c

        d
    e

f
    g
\th
";

#[test]
fn the_blocks_enclosing_the_rows_come_outermost_first() {
    let text = Text::new(TEXT);
    // a holds 1-5 and b 2-4, the blank row inside it; the blank row after
    // e trails a. f's block holds g and the tab-indented h.
    assert_eq!(all_blocks(&text, 4), [0..=5, 1..=4, 7..=9]);
    assert_eq!(indent_scopes(&text, 2..5, 4), [0..=5, 1..=4]);
    // A header row is inside its own block, and a blank row in a block is
    // too.
    assert_eq!(indent_scopes(&text, 1..5, 4), [0..=5, 1..=4]);
    assert_eq!(indent_scopes(&text, 3..5, 4), [0..=5, 1..=4]);
    // Rows covering the end of a and f's block, to the end of the file.
    assert_eq!(indent_scopes(&text, 5..10, 4), [0..=5, 7..=9]);
    assert_eq!(indent_scopes(&text, 8..10, 4), [7..=9]);
    // A trailing blank row is in no block, and f's isn't known yet.
    assert_eq!(indent_scopes(&text, 6..7, 4), []);
    // Still open at the row past the rows: ending there.
    assert_eq!(indent_scopes(&text, 1..3, 4), [0..=3, 1..=3]);
    assert_eq!(indent_scopes(&text, 2..2, 4), []);
    assert_eq!(indent_scopes(&text, 40..50, 4), []);
}

#[test]
fn every_window_matches_the_whole_files_blocks() {
    let samples = [
        TEXT.to_owned(),
        "x\n  y\n\n\n  z\n\n\nw\n".to_owned(),
        "  starts indented\n    deeper\n  back\nnone\n\n".to_owned(),
        "\n\n\nfn f() {\n    if a {\n        b();\n    }\n\n}\n\n\n".to_owned(),
        "a\n\tb\n  \tc\n\t\td\n e\n".to_owned(),
    ];
    for sample in &samples {
        let text = Text::new(sample);
        let rows = text.row_count();
        for tab_size in [2, 4] {
            for start in 0..rows {
                for len in 1..=6 {
                    let window = start..(start + len).min(rows);
                    assert_eq!(
                        indent_scopes(&text, window.clone(), tab_size),
                        expected(&text, window.clone(), tab_size),
                        "{sample:?} {window:?} tab {tab_size}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_block_open_past_the_rows_of_a_million_row_file_is_found_without_scanning_on() {
    let mut text = String::from("root:\n    first:\n");
    text.push_str(&"        x\n".repeat(1_000_000));
    let text = Text::new(&text);
    let before = MEASURED.get();
    assert_eq!(indent_scopes(&text, 3..10, 4), [0..=10, 1..=10]);
    // The rows and the one past them, then the three above.
    assert_eq!(MEASURED.get() - before, 8 + 3);
}

#[test]
fn headers_are_found_a_window_at_a_time_up_to_the_scan_limit() {
    // Sections of 1,000 rows, each a header and its block.
    let mut text = String::new();
    for i in 0..1_000 {
        text.push_str(&format!("section {i}\n"));
        text.push_str(&"    x\n".repeat(999));
    }
    let text = Text::new(&text);
    let before = MEASURED.get();
    let header = 500_000;
    assert_eq!(
        indent_scopes(&text, header + 500..header + 507, 4),
        [header..=header + 507]
    );
    // 8 rows, then windows of 64, 256 and 1,024 rows above them.
    assert_eq!(MEASURED.get() - before, 8 + 64 + 256 + 1_024);

    // A header further up than the limit isn't found.
    let mut text = String::from("root:\n");
    text.push_str(&"    x\n".repeat(60_000));
    let text = Text::new(&text);
    let near = INDENT_SCAN_ROWS;
    assert_eq!(indent_scopes(&text, near..near + 7, 4), [0..=near + 7]);
    let before = MEASURED.get();
    assert_eq!(indent_scopes(&text, near + 1..near + 8, 4), []);
    assert_eq!(MEASURED.get() - before, 8 + INDENT_SCAN_ROWS as usize);
}

#[test]
fn indents_are_read_in_one_pass_as_row_by_row() {
    let long = format!("  {}\n", "y".repeat(100_000));
    let text = format!("a\n\t b\n\u{3000} c\n   \n{long}\t\t\n  \u{e9}\n  last");
    let text = Text::new(&text);
    let rows = 0..text.row_count();
    let all = indents(&text, rows.clone(), 4);
    assert_eq!(
        all,
        [
            Some(0),
            Some(5),
            Some(1),
            None,
            Some(2),
            None,
            Some(2),
            Some(2)
        ]
    );
    for start in rows.clone() {
        for end in start..=rows.end {
            let window = indents(&text, start..end, 4);
            assert_eq!(window, all[start as usize..end as usize], "{start}..{end}");
        }
    }
}
