//! `scrolloff`: how many rows autoscroll keeps between the caret and the
//! window's edges, against the fake monospace measurer of [`super::tests`].

use fe_buffer::Buffer;

use super::{
    tests::{CELL, LINE, Mono, WIDTH},
    *,
};

/// Rows in the window.
const ROWS: u32 = 20;

/// A window of `rows` rows over 100 rows of text with `scroll_off`, its
/// caret at the start of buffer row `caret`, autoscrolled from `scroll_y`;
/// returns the scroll it ends at, in rows.
fn scrolled(scroll_off: u32, rows: u32, caret: u32, scroll_y: f32) -> f32 {
    let text = (0..100).map(|k| format!("{k}\n")).collect::<String>();
    let buffer = Buffer::new(&text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    display.set_scroll_off(scroll_off);
    assert_eq!(display.scroll_off(), scroll_off);
    let head = display.text().row_start(caret);
    let selections = [ResolvedSelection {
        range: head..head,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: head,
        scroll: Vec2 { x: 0., y: scroll_y },
        autoscroll: true,
        width: WIDTH,
        height: LINE * rows as f32,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    let frame = layout_frame(
        &input,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    frame.scroll.y / LINE
}

#[test]
fn caret_keeps_n_rows_from_the_edges() {
    for scroll_off in [0, 3, 8] {
        let n = scroll_off as f32;
        // Moved below the window, from the top: the caret's row is `n`
        // rows from the bottom edge.
        let scroll = scrolled(scroll_off, ROWS, 40, 0.);
        assert_eq!(scroll, 40. + 1. + n - ROWS as f32, "below, {scroll_off}");
        // Moved above the window, from further down: `n` rows from the top.
        let scroll = scrolled(scroll_off, ROWS, 10, 60. * LINE);
        assert_eq!(scroll, 10. - n, "above, {scroll_off}");
        // Already within the margins: left alone.
        let scroll = scrolled(scroll_off, ROWS, 30, 20. * LINE);
        assert_eq!(scroll, 20., "inside, {scroll_off}");
    }
    // 3 is today's: a third of the window at most, the default's cap.
    assert_eq!(scrolled(3, 6, 40, 0.), 40. + 1. + 2. - 6.);
}

#[test]
fn huge_and_tiny() {
    // More than half the window's rows centres the caret: the same for 11,
    // 100 and 999, half a row off the middle of 20.
    for scroll_off in [11, 100, 999] {
        assert_eq!(
            scrolled(scroll_off, ROWS, 40, 0.),
            40. - 9.5,
            "{scroll_off}"
        );
        assert_eq!(scrolled(scroll_off, ROWS, 10, 60. * LINE), 10. - 9.5);
    }
    // The last row is centred, not scrolled to the top: never past it, nor
    // above the first.
    assert_eq!(scrolled(999, ROWS, 99, 0.), 99. - 9.5);
    assert_eq!(scrolled(999, ROWS, 0, 50. * LINE), 0.);
    // A window of two rows or one neither loops nor panics, the caret near it (the default's third of
    // a window of one row leaves a third of a row off).
    for rows in [1, 2] {
        for scroll_off in [0, 3, 999] {
            let scroll = scrolled(scroll_off, rows, 40, 0.);
            assert!(
                (40. - rows as f32..=41.).contains(&scroll),
                "{rows} {scroll_off}: {scroll}"
            );
        }
    }
}
