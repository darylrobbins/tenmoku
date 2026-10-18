//! Inlay hints in a frame, against the fake measurer of the layout tests:
//! any number of texts drawn inside a row, beside the one inlay, with the
//! rest of the row shifted past each.

use fe_buffer::Buffer;

use super::{tests::*, *};

fn input<'a>(
    display: &'a DisplayMap,
    selections: &'a [ResolvedSelection],
    scroll_x: f32,
) -> FrameInput<'a, &'static str> {
    FrameInput {
        display,
        tab_size: 4,
        selections,
        primary_head: selections[0].head(),
        scroll: Vec2 { x: scroll_x, y: 0. },
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
    }
}

fn hints(hints: &[(usize, &str)]) -> Vec<Inlay<&'static str>> {
    (hints.iter())
        .map(|&(offset, text)| Inlay {
            offset,
            text: text.to_string(),
            style: "hint",
        })
        .collect()
}

fn lay_out_hints(
    marked: &str,
    inlay: Option<(usize, &str)>,
    hint_list: &[(usize, &str)],
) -> (Frame<Line>, Buffer) {
    let (text, selections) = parse(marked);
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    let inlay = inlay.map(|(offset, text)| Inlay {
        offset,
        text: text.to_string(),
        style: "ghost",
    });
    let frame = layout_frame_with_hints(
        &input(&display, &selections, 0.),
        &BlockMap::default(),
        inlay.as_ref(),
        &hints(hint_list),
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    (frame, buffer)
}

fn lay_out_plain(marked: &str) -> Frame<Line> {
    let (text, selections) = parse(marked);
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    layout_frame_with_inlay(
        &input(&display, &selections, 0.),
        &BlockMap::default(),
        None,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

fn x_for(frame: &Frame<Line>, offset: usize) -> f64 {
    let row = frame.row_containing(offset).unwrap();
    row.x_for_offset(offset, &mut LongRows::default(), &mut Mono)
}

/// The offset under `x` px right of the text's left edge, in the first row.
fn offset_at(frame: &Frame<Line>, buffer: &Buffer, x: f32) -> usize {
    frame.offset_at(
        buffer.current().text(),
        Vec2 {
            x: GUTTER + x,
            y: 5.,
        },
        &mut LongRows::default(),
        &mut Mono,
    )
}

const ROW: &str = "let a = f(1|);\nnext\nlast";
/// `: u8` after the binding and `x: ` before the argument, given out of
/// order.
const HINTS: [(usize, &str); 2] = [(10, "x: "), (5, ": u8")];

#[test]
fn hints_shift_the_rest_of_their_row() {
    let (frame, _) = lay_out_hints(ROW, None, &HINTS);
    let row = &frame.rows[0];
    assert_eq!(row.line.text, "let a: u8 = f(x: 1);");
    let runs: Vec<_> = (row.line.runs.iter())
        .map(|run| (run.len, run.style))
        .collect();
    assert_eq!(
        runs,
        [
            (5, "text"),
            (4, "hint"),
            (5, "text"),
            (3, "hint"),
            (3, "text")
        ]
    );
    assert_eq!(row.hints, [1, 0]);
    assert_eq!(row.inlay_width, 7. * f64::from(CELL));

    // Each hint is at its column's x plus the hints before it.
    assert_eq!(
        frame.hints,
        [
            (1, rect(GUTTER + 5. * CELL, 0., 4. * CELL, LINE)),
            (0, rect(GUTTER + 14. * CELL, 0., 3. * CELL, LINE)),
        ]
    );
    assert_eq!(frame.inlay, None);

    // Every later offset shifts by the hints before it.
    let cell = f64::from(CELL);
    for offset in 0..=13 {
        let shift = match offset {
            0..=5 => 0,
            6..=10 => 4,
            _ => 7,
        };
        assert_eq!(
            x_for(&frame, offset),
            (offset + shift) as f64 * cell,
            "{offset}"
        );
    }
    assert_eq!(frame.cursors, [rect(GUTTER + 18. * CELL, 0., 2., LINE)]);

    // Rows without hints are as they are laid out without any.
    let plain = lay_out_plain(ROW);
    assert_eq!(plain.rows[0].line.text, "let a = f(1);");
    for ix in 1..3 {
        let (with, without) = (&frame.rows[ix], &plain.rows[ix]);
        assert_eq!(with.line.text, without.line.text);
        assert_eq!(with.line.runs, without.line.runs);
        assert!(with.hints.is_empty());
        assert_eq!(with.inlay_width, 0.);
        for offset in with.start..=with.end {
            assert_eq!(x_for(&frame, offset), x_for(&plain, offset));
        }
    }
    assert_eq!(frame.line_numbers.len(), plain.line_numbers.len());
}

#[test]
fn the_caret_never_lands_inside_a_hint() {
    let (frame, buffer) = lay_out_hints(ROW, None, &HINTS);
    for &(ix, rect) in &frame.hints {
        let offset = HINTS[ix].0;
        let left = rect.x - GUTTER;
        // Every x on the hint, its edges too, gives the hint's offset.
        let mut x = left;
        while x <= left + rect.width {
            assert_eq!(
                offset_at(&frame, &buffer, x),
                offset,
                "{x} on {:?}",
                HINTS[ix]
            );
            x += 1.;
        }
        // The offset is before the hint, the next one after it.
        assert_eq!(x_for(&frame, offset), f64::from(left));
        assert_eq!(
            x_for(&frame, offset + 1),
            f64::from(left + rect.width + CELL)
        );
    }
    // Either side of a hint, a click finds the char it's on.
    assert_eq!(offset_at(&frame, &buffer, 4. * CELL + 2.), 4);
    assert_eq!(offset_at(&frame, &buffer, 9. * CELL + 2.), 5);
    assert_eq!(offset_at(&frame, &buffer, 10. * CELL + 2.), 6);
    assert_eq!(offset_at(&frame, &buffer, 13. * CELL + 2.), 9);
    assert_eq!(offset_at(&frame, &buffer, 17. * CELL + 2.), 10);
    assert_eq!(offset_at(&frame, &buffer, 18. * CELL + 2.), 11);
}

#[test]
fn hints_and_the_inlay_share_a_row() {
    let (frame, buffer) = lay_out_hints("ab|cd", Some((2, "XYZ")), &[(2, "H:"), (3, "J")]);
    let row = &frame.rows[0];
    // The hint goes first at the inlay's offset.
    assert_eq!(row.line.text, "abH:XYZcJd");
    let runs: Vec<_> = (row.line.runs.iter())
        .map(|run| (run.len, run.style))
        .collect();
    assert_eq!(
        runs,
        [
            (2, "text"),
            (2, "hint"),
            (3, "ghost"),
            (1, "text"),
            (1, "hint"),
            (1, "text")
        ]
    );
    assert_eq!(
        frame.hints,
        [
            (0, rect(GUTTER + 2. * CELL, 0., 2. * CELL, LINE)),
            (1, rect(GUTTER + 8. * CELL, 0., CELL, LINE)),
        ]
    );
    assert_eq!(
        frame.inlay,
        Some(rect(GUTTER + 4. * CELL, 0., 3. * CELL, LINE))
    );
    assert_eq!(row.inlay_width, 6. * f64::from(CELL));
    let cell = f64::from(CELL);
    assert_eq!(x_for(&frame, 2), 2. * cell);
    assert_eq!(x_for(&frame, 3), 8. * cell);
    assert_eq!(x_for(&frame, 4), 10. * cell);
    // On the hint or the inlay, a click gives their offset.
    for x in [25., 45., 65.] {
        assert_eq!(offset_at(&frame, &buffer, x), 2, "{x}");
    }
    assert_eq!(offset_at(&frame, &buffer, 85.), 3);
    assert_eq!(frame.cursors, [rect(GUTTER + 2. * CELL, 0., 2., LINE)]);
}

#[test]
fn long_row_mappings_count_every_hint_before_the_slice() {
    // 9000 `x`, scrolled to offset 5000, with hints before the slice, in it
    // and after it.
    let hint_list = [(40, "AB"), (3000, "CDE"), (5010, "FG"), (8000, "HIJ")];
    let buffer = Buffer::new(&"x".repeat(9000));
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let mut long_rows = LongRows::default();
    let cell = f64::from(CELL);
    let scroll_x = 50_000.;
    let frame = layout_frame_with_hints(
        &input(&display, &selections, scroll_x),
        &BlockMap::default(),
        None,
        &hints(&hint_list),
        &mut Mono,
        &mut long_rows,
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    assert_eq!(frame.scroll.x, scroll_x);
    let row = &frame.rows[0];
    let shown = row.shown();
    assert!(shown.start > 3000 && shown.end < 8000, "{shown:?}");
    assert!(shown.contains(&5010), "{shown:?}");
    // The hints before the slice move it right by their widths.
    assert_eq!(row.start_x(), (shown.start + 5) as f64 * cell);
    // The view's left edge shows offset 4995, past the 5 cells of hints.
    assert_eq!(
        frame.offset_at(
            buffer.current().text(),
            Vec2 {
                x: GUTTER + 2.,
                y: 5.
            },
            &mut long_rows,
            &mut Mono,
        ),
        4995
    );
    assert_eq!(row.hints, [2]);
    assert_eq!(frame.hints, [(2, rect(GUTTER + 150., 0., 2. * CELL, LINE))]);

    // Each offset is right of where the index puts it by every hint before it.
    let expected = |offset: usize| {
        let before: usize = (hint_list.iter())
            .filter(|(at, _)| *at < offset)
            .map(|(_, text)| text.len())
            .sum();
        (offset + before) as f64 * cell
    };
    let mut x_at = |offset: usize| row.x_for_offset(offset, &mut long_rows, &mut Mono);
    let outside = [0, 40, 41, 2999, 3000, 3001, 7999, 8000, 8001, 8999, 9000];
    for offset in shown.clone().chain(outside) {
        assert_eq!(x_at(offset), expected(offset), "{offset}");
    }

    // A click just right of an offset's x finds it, in the slice and out.
    let mut at = |x: f64| {
        frame.offset_at(
            buffer.current().text(),
            Vec2 {
                x: frame.view_x(x) + 2.,
                y: 5.,
            },
            &mut long_rows,
            &mut Mono,
        )
    };
    for offset in shown.clone().chain(outside) {
        assert_eq!(at(expected(offset)), offset, "{offset}");
    }
    // On a hint outside the slice, a click gives its offset.
    for (offset, text) in [(40, "AB"), (3000, "CDE"), (8000, "HIJ")] {
        let left = expected(offset);
        for x in 0..text.len() * 10 {
            assert_eq!(at(left + x as f64 - 2.), offset, "{x} on {text}");
        }
    }
}

#[test]
fn long_row_scroll_limit_counts_every_hint() {
    // An excessive scroll stops with the row's end, past all the hints, 4
    // ems from the view's right edge.
    let buffer = Buffer::new(&"x".repeat(9000));
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let frame = layout_frame_with_hints(
        &input(&display, &selections, 1e9),
        &BlockMap::default(),
        None,
        &hints(&[(40, "AB"), (8990, "CDE")]),
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    let text_width = WIDTH - GUTTER;
    assert_eq!(frame.scroll.x, 90_050. - text_width + 4. * CELL);
    assert!(frame.rows[0].line.text.contains("CDE"));
}

#[test]
fn hints_lay_out_without_a_gutter_too() {
    let (text, selections) = parse(ROW);
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    let frame = layout_frame_without_gutter_with_hints(
        &input(&display, &selections, 0.),
        &BlockMap::default(),
        None,
        &hints(&HINTS),
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
    );
    assert_eq!(frame.rows[0].line.text, "let a: u8 = f(x: 1);");
    let left = CELL * NO_GUTTER_LEFT_EMS;
    assert_eq!(
        frame.hints,
        [
            (1, rect(left + 5. * CELL, 0., 4. * CELL, LINE)),
            (0, rect(left + 14. * CELL, 0., 3. * CELL, LINE)),
        ]
    );
    assert_eq!(frame.cursors, [rect(left + 18. * CELL, 0., 2., LINE)]);
}

#[path = "long_row_hint_inlay_tests.rs"]
mod long_row_hint_inlay_tests;
