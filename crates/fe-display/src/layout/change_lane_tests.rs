//! The change lane, between the line numbers and the fold column, against
//! the fake monospace measurer of [`super::tests`] (em 10).

use fe_buffer::Buffer;

use super::{
    tests::{CELL, LINE, Line, Mono, WIDTH},
    *,
};

const HEIGHT: f32 = 200.;

/// Lays out `rows` numbered rows scrolled to `scroll_y`, every row heading a
/// fold candidate.
fn lay_out(rows: u32, scroll_y: f32) -> Frame<Line> {
    let text = (0..rows)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n");
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2 { x: 0., y: scroll_y },
        autoscroll: false,
        width: WIDTH,
        height: HEIGHT,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    layout_frame(
        &input,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |rows| rows.collect(),
    )
}

#[test]
fn the_constants_keep_their_values() {
    assert_eq!(LINE_NUMBER_GAP_EMS, 0.5);
    assert_eq!(CHANGE_LANE_EMS, 1.0);
    assert_eq!(FOLD_COLUMN_EMS, 3.25);
    assert_eq!(FOLD_CHEVRON_EMS, 2.25);
}
