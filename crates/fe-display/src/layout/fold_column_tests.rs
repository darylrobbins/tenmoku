//! The fold column and its chevron boxes, against the fake monospace
//! measurer of [`super::tests`] (em 10).

use fe_buffer::Buffer;

use super::{
    tests::{CELL, Line, Mono, WIDTH},
    *,
};

/// Lays out `text` in a 400 × 200 editor with rows `line_height` tall, row 2
/// folded (hiding row 3) and rows 0 and 4 heading fold candidates.
fn lay_out(text: &str, line_height: f32) -> Frame<Line> {
    let buffer = Buffer::new(text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    display.fold(2..=3);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2::default(),
        autoscroll: false,
        width: WIDTH,
        height: 200.,
        line_height,
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
        |rows| {
            [0, 4]
                .into_iter()
                .filter(|row| rows.contains(row))
                .collect()
        },
    )
}

const TEXT: &str = "a {\n}\nb {\n  x\n}\nc {\n}";

fn centre(rect: Rect) -> Vec2 {
    Vec2 {
        x: rect.x + rect.width / 2.,
        y: rect.y + rect.height / 2.,
    }
}

#[test]
fn the_column_pads_the_chevron() {
    let frame = lay_out(TEXT, 32.);
    // em × (3 digits + 6.75).
    assert_eq!(frame.text_left, 97.5);
    let column = Rect {
        x: 55.,
        y: 0.,
        width: 37.5,
        height: 200.,
    };
    assert_eq!(frame.fold_column, column);
    // Line numbers end 0.5 em left of the fold column, which is flush
    // against its left edge.
    for (origin, line) in &frame.line_numbers {
        assert_eq!(origin.x + line.width(), column.x - 5.);
    }

    // Row 2 is folded; rows 0 and 4 (display row 3) are candidates.
    let toggles: Vec<_> = frame
        .fold_toggles
        .iter()
        .map(|toggle| (toggle.buffer_row, toggle.folded))
        .collect();
    assert_eq!(toggles, [(0, false), (2, true), (4, false)]);
    for (toggle, display_row) in frame.fold_toggles.iter().zip([0., 2., 3.]) {
        let row = Rect {
            x: column.x,
            y: display_row * 32.,
            width: column.width,
            height: 32.,
        };
        assert_eq!(toggle.rect, row);
        let chevron = frame.chevron_box(toggle);
        assert_eq!((chevron.width, chevron.height), (22.5, 22.5));
        assert_eq!(centre(chevron), centre(row));
        // 0.75 em on each side.
        assert_eq!(chevron.x - column.x, 7.5);
        assert_eq!(column.x + column.width - (chevron.x + chevron.width), 7.5);
    }
}

#[test]
fn the_chevron_fits_short_rows() {
    let frame = lay_out(TEXT, 20.);
    assert_eq!(frame.fold_chevron, 17.5);
    for toggle in &frame.fold_toggles {
        let chevron = frame.chevron_box(toggle);
        assert_eq!((chevron.width, chevron.height), (17.5, 17.5));
        assert_eq!(centre(chevron), centre(toggle.rect));
        assert_eq!(centre(chevron).x, frame.fold_column.x + 37.5 / 2.);
    }
}
