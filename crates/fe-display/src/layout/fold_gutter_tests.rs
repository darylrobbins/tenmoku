//! Fold toggles in the gutter and fold markers' hit rectangles, against the
//! fake monospace measurer of [`super::tests`].

use fe_buffer::Buffer;

use super::{
    tests::{CELL, GUTTER, LINE, Line, Mono, WIDTH, rect},
    *,
};

/// Lays out `text` in a 400 × `height` editor scrolled right by `scroll_x`,
/// its rows wrapped or folded by `configure`, with the rows `candidates`
/// lists heading fold candidates. Returns the frame and the buffer rows the
/// candidates were asked for.
fn lay_out(
    text: &str,
    (scroll_x, height): (f32, f32),
    configure: impl FnOnce(&mut DisplayMap),
    candidates: &[u32],
) -> (Frame<Line>, Vec<Range<u32>>) {
    let buffer = Buffer::new(text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    configure(&mut display);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2 { x: scroll_x, y: 0. },
        autoscroll: false,
        width: WIDTH,
        height,
        line_height: LINE,
        em: CELL,
        styles: FrameStyles {
            text: "text",
            line_number: "number",
            active_line_number: "active",
        },
    };
    let mut asked = Vec::new();
    let frame = layout_frame(
        &input,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |rows| {
            asked.push(rows.clone());
            candidates
                .iter()
                .copied()
                .filter(|row| rows.contains(row))
                .collect()
        },
    );
    (frame, asked)
}

#[test]
fn toggles_for_candidates_and_folds() {
    let text = "a {\n  x\n}\nb {\n  y\n}\ncccc dddd eeee\nz\nw {\n  v\n}";
    let configure = |display: &mut DisplayMap| {
        display.set_wrap_width(Some(6.), &mut crate::Monospace);
        display.wrap_pending(0, usize::MAX, &mut crate::Monospace);
        // Row 3 heads a fold and a candidate; row 8 only a fold.
        display.fold(3..=4);
        display.fold(8..=9);
    };
    let (frame, asked) = lay_out(text, (0., 240.), configure, &[0, 3, 6]);
    // Rows 0–10, less the folded 4 and 9, fit.
    assert_eq!(asked, vec![0..11]);
    let toggle = |display_row: u32, buffer_row: u32, folded: bool| FoldToggle {
        rect: rect(
            GUTTER - 2. * CELL,
            display_row as f32 * LINE,
            2. * CELL,
            LINE,
        ),
        buffer_row,
        folded,
    };
    // Row 6 wraps onto display rows 5–7; only its first has a toggle.
    assert_eq!(
        frame.fold_toggles,
        [
            toggle(0, 0, false),
            toggle(3, 3, true),
            toggle(5, 6, false),
            toggle(9, 8, true),
        ]
    );
    // Between the line number and the text.
    let number = &frame.line_numbers[0];
    assert!(number.0.x + number.1.width() <= frame.fold_toggles[0].rect.x);
    let toggle = frame.fold_toggles[0].rect;
    assert_eq!(toggle.x + toggle.width, frame.text_left);
}

#[test]
fn markers_have_hit_rects() {
    let configure = |display: &mut DisplayMap| {
        display.fold(0..=1);
        display.fold(3..=4);
    };
    let (frame, _) = lay_out("abc\n  x\ny\nlonger {\n  z\n}", (0., 240.), configure, &[]);
    let markers: Vec<_> = frame
        .fold_markers
        .iter()
        .map(|marker| (marker.rect, marker.buffer_row, marker.line.text.as_str()))
        .collect();
    // A cell after each header's text, as wide and tall as the marker.
    assert_eq!(
        markers,
        [
            (rect(GUTTER + 40., 0., CELL, LINE), 0, FOLD_MARKER),
            (rect(GUTTER + 90., 2. * LINE, CELL, LINE), 3, FOLD_MARKER),
        ]
    );
    for marker in &frame.fold_markers {
        assert_eq!(marker.rect.width, marker.line.width());
    }
}

#[test]
fn click_areas_are_clipped_to_what_shows() {
    let text = "x".repeat(100) + "\nabc\n  x\ny {\n  z\n}";
    let configure = |display: &mut DisplayMap| {
        display.fold(1..=2);
    };
    let areas = |scroll_x: f32| {
        // Display rows 0–2 show, the last only half.
        let (frame, _) = lay_out(&text, (scroll_x, 50.), configure, &[3]);
        frame.fold_click_areas().collect::<Vec<_>>()
    };
    let area = |rect, buffer_row, folded| FoldToggle {
        rect,
        buffer_row,
        folded,
    };
    let toggles = [
        area(rect(GUTTER - 2. * CELL, LINE, 2. * CELL, LINE), 1, true),
        area(
            rect(GUTTER - 2. * CELL, 2. * LINE, 2. * CELL, 10.),
            3,
            false,
        ),
    ];
    // Half the marker after `abc` is scrolled under the gutter.
    let marker = area(rect(GUTTER, LINE, 5., LINE), 1, true);
    assert_eq!(areas(45.), [toggles[0], toggles[1], marker]);
    // All of it is.
    assert_eq!(areas(60.), toggles);
}
