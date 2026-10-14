//! The change lane, between the fold column and the text, against the fake
//! monospace measurer of [`super::tests`] (em 10).

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

/// The 3 px change bar fe-gpui draws at the lane's left edge.
const BAR: f32 = 3.;

#[test]
fn order() {
    for (rows, digits) in [(500, 3.), (5000, 4.), (50_000, 5.)] {
        let gutter = CELL * (digits + 2. + 0.5 + 1.0 + 3.25);
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
                x: frame.text_left - CELL,
                y: 0.,
                width: CELL,
                height: HEIGHT,
            };
            assert_eq!(frame.change_lane, lane, "{label}");
            assert_eq!(
                frame.fold_column.x + frame.fold_column.width,
                frame.change_lane.x,
                "{label}"
            );
            assert!(!frame.line_numbers.is_empty(), "{label}");
            for (origin, line) in &frame.line_numbers {
                assert_eq!(
                    origin.x + line.width(),
                    frame.fold_column.x - 0.5 * CELL,
                    "{label}"
                );
            }
        }
    }
}

#[test]
fn no_overlap() {
    for rows in [500, 5000, 50_000] {
        for scroll_y in [0., rows as f32 * LINE] {
            let label = format!("{rows} rows, scrolled to {scroll_y}");
            let frame = lay_out(rows, scroll_y);
            let column = frame.fold_column;
            let lane = frame.change_lane;
            let numbers_right = frame
                .line_numbers
                .iter()
                .map(|(origin, line)| origin.x + line.width())
                .fold(f32::MIN, f32::max);
            assert!(!frame.fold_toggles.is_empty(), "{label}");
            for toggle in &frame.fold_toggles {
                let chevron = frame.chevron_box(toggle);
                for rect in [toggle.rect, chevron] {
                    assert!(rect.x >= column.x, "{label}: {rect:?}");
                    assert!(rect.x + rect.width <= column.x + column.width, "{label}");
                    assert!(rect.x >= numbers_right, "{label}: {rect:?}");
                    assert!(rect.x + rect.width <= lane.x, "{label}: {rect:?}");
                }
                // A 3 px bar at the lane's left edge stays off the text, and
                // is 0.5 em clear of the chevron.
                assert!(lane.x + BAR <= frame.text_left, "{label}");
                assert!(
                    lane.x - (chevron.x + chevron.width) >= 0.5 * CELL,
                    "{label}: {chevron:?}"
                );
            }
        }
    }
}

#[test]
fn a_bar_at_the_lane_edge_clears_the_text_by_0_6_em() {
    for rows in [500, 5000, 50_000] {
        for scroll_y in [0., rows as f32 * LINE] {
            let label = format!("{rows} rows, scrolled to {scroll_y}");
            let frame = lay_out(rows, scroll_y);
            let lane = frame.change_lane;
            assert!(lane.x + BAR + 0.6 * CELL <= frame.text_left, "{label}");
            assert_eq!(lane.width, CELL, "{label}");
            assert_eq!(lane.x + lane.width, frame.text_left, "{label}");
            assert!(!frame.fold_toggles.is_empty(), "{label}");
            for toggle in &frame.fold_toggles {
                let chevron = frame.chevron_box(toggle);
                assert!(
                    chevron.x + chevron.width + 0.5 * CELL <= lane.x,
                    "{label}: {chevron:?}"
                );
            }
        }
    }
}
