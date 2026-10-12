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
    assert_eq!(CHANGE_GAP_EMS, 0.5);
    assert_eq!(CHANGE_LANE_EMS, 0.5);
    assert_eq!(FOLD_COLUMN_EMS, 3.75);
    assert_eq!(FOLD_CHEVRON_EMS, 2.25);
}

#[test]
fn the_lane_sits_clear_of_the_numbers_and_the_chevrons() {
    for (rows, digits) in [(500, 3.), (5000, 4.), (50_000, 5.)] {
        let gutter = CELL * (digits + 2. + CHANGE_GAP_EMS + CHANGE_LANE_EMS + FOLD_COLUMN_EMS);
        assert_eq!(gutter_width(rows, CELL), gutter, "{rows} rows");
        assert_eq!(
            wrap_width(WIDTH, rows, CELL),
            ((WIDTH - gutter - 2. * CELL) / CELL).floor() * CELL,
            "{rows} rows"
        );
        // The first rows and the last, whose numbers are widest.
        for scroll_y in [0., rows as f32 * LINE] {
            let label = format!("{rows} rows, scrolled to {scroll_y}");
            let frame = lay_out(rows, scroll_y);
            assert_eq!(frame.text_left, gutter, "{label}");
            let lane = Rect {
                x: frame.fold_column.x - 5.,
                y: 0.,
                width: 5.,
                height: HEIGHT,
            };
            assert_eq!(frame.change_lane, lane, "{label}");
            assert_eq!(frame.fold_column.x, gutter - CELL * FOLD_COLUMN_EMS);

            assert!(!frame.line_numbers.is_empty(), "{label}");
            for (origin, line) in &frame.line_numbers {
                assert_eq!(origin.x + line.width(), lane.x - 5., "{label}");
            }
            // A 3 px bar at the lane's left edge is 0.5 em clear of the
            // chevrons.
            assert!(!frame.fold_toggles.is_empty(), "{label}");
            for toggle in &frame.fold_toggles {
                let chevron = frame.chevron_box(toggle);
                assert!(chevron.x - (lane.x + 3.) >= 5., "{label}: {chevron:?}");
            }
        }
    }
}
