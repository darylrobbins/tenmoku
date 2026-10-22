//! The x a vim block cursor starts at, past the hints at its offset, against
//! the fake measurer of the layout tests.

use fe_buffer::Buffer;

use super::{tests::*, *};

/// The frame for `marked` with `hint_list`, and the block's span `(start, end)`
/// for a caret at `offset`: from where its character starts to the next
/// grapheme's x, as the editor draws it.
fn block(marked: &str, hint_list: &[(usize, &str)], offset: usize) -> (f64, f64) {
    block_with_ghost(marked, hint_list, None, offset)
}

/// [`block`] with ghost text drawn at the caret, which the end leaves out.
fn block_with_ghost(
    marked: &str,
    hint_list: &[(usize, &str)],
    ghost: Option<(usize, &str)>,
    offset: usize,
) -> (f64, f64) {
    let (text, selections) = parse(marked);
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    let hints: Vec<Inlay<&'static str>> = (hint_list.iter())
        .map(|&(offset, text)| Inlay {
            offset,
            text: text.to_string(),
            style: "hint",
        })
        .collect();
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: selections[0].head(),
        scroll: Vec2 { x: 0., y: 0. },
        autoscroll: false,
        width: WIDTH,
        height: 100.,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    let frame = layout_frame_with_hints(
        &input,
        &BlockMap::default(),
        ghost
            .map(|(offset, text)| Inlay {
                offset,
                text: text.to_string(),
                style: "ghost",
            })
            .as_ref(),
        &hints,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    let row = frame.row_containing(offset).unwrap();
    let long_rows = &mut LongRows::default();
    let start = row.x_after_hints(offset, long_rows, &mut Mono);
    let end = if offset < row.end {
        row.x_for_offset(offset + 1, long_rows, &mut Mono) - row.ghost_width()
    } else {
        start + f64::from(CELL)
    };
    (start, end)
}

fn cells(n: usize) -> f64 {
    n as f64 * f64::from(CELL)
}

#[test]
fn block_starts_after_the_hint_at_the_caret_offset() {
    // `f(order: Ordering)`: the hint is drawn before the `O` at offset 2.
    let (start, end) = block("f(|O);", &[(2, "order: ")], 2);
    assert_eq!(start, cells(2 + 7));
    assert_eq!(end, cells(2 + 7 + 1));
    assert_eq!(end - start, f64::from(CELL));
}

#[test]
fn a_caret_without_a_hint_keeps_its_block() {
    let (start, end) = block("abc|d", &[(1, "x: ")], 3);
    assert_eq!(start, cells(3 + 3));
    assert_eq!(end - start, f64::from(CELL));
    let (start, end) = block("a|bcd", &[(3, "x: ")], 1);
    assert_eq!(start, cells(1));
    assert_eq!(end, cells(2));
}

#[test]
fn a_caret_after_two_hints_starts_after_both() {
    let (start, end) = block("ab|cd", &[(2, "x: "), (2, "y: ")], 2);
    assert_eq!(start, cells(2 + 6));
    assert_eq!(end - start, f64::from(CELL));
}

#[test]
fn a_hint_after_the_character_is_not_covered() {
    // `let batch: u64`, the caret on `h` at offset 7.
    let (start, end) = block("let batc|h = 1;", &[(9, ": u64")], 8);
    assert_eq!(start, cells(8));
    assert_eq!(end, cells(9));
}

#[test]
fn a_caret_at_the_end_of_the_row_draws_one_cell_after_the_hint() {
    let (start, end) = block("ab|", &[(2, ": u8")], 2);
    assert_eq!(start, cells(2 + 4));
    assert_eq!(end - start, f64::from(CELL));
}

#[test]
fn ghost_text_beside_a_hint_leaves_the_tab_its_width() {
    // A tab at offset 1 (three cells, to the tab stop), with a one-cell hint and a two-cell ghost.
    let (start, end) = block_with_ghost("a|\tb", &[(1, "h")], Some((1, "gg")), 1);
    assert_eq!(start, cells(1 + 1));
    assert_eq!(end - start, cells(3));
}
