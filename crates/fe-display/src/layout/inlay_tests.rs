//! One inlay in a frame, against the fake measurer of the layout tests:
//! text drawn inside a row, with the rest of the row shifted past it.

use fe_buffer::Buffer;

use super::{tests::*, *};

fn lay_out_inlay(marked: &str, inlay: Option<(usize, &str)>) -> Frame<Line> {
    let (text, selections) = parse(marked);
    let buffer = Buffer::new(&text);
    let display = DisplayMap::new(buffer.current(), 4);
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: selections[0].head(),
        scroll: Vec2::default(),
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
    let inlay = inlay.map(|(offset, text)| Inlay {
        offset,
        text: text.to_string(),
        style: "ghost",
    });
    layout_frame_with_inlay(
        &input,
        &BlockMap::default(),
        inlay.as_ref(),
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

#[test]
fn inlay_shifts_the_rest_of_its_row() {
    let frame = lay_out_inlay("|abcd", Some((2, "XYZ")));
    let row = &frame.rows[0];
    assert_eq!(row.line.text, "abXYZcd");
    assert_eq!(x_for(&frame, 2), 2. * f64::from(CELL));
    assert_eq!(x_for(&frame, 3), 6. * f64::from(CELL));
    assert_eq!(x_for(&frame, 4), 7. * f64::from(CELL));
    let text = Buffer::new("abcd");
    let at = |x: f32, frame: &Frame<Line>| {
        frame.offset_at(
            text.current().text(),
            Vec2 {
                x: GUTTER + x,
                y: 5.,
            },
            &mut LongRows::default(),
            &mut Mono,
        )
    };
    assert_eq!(at(35., &frame), 2);
    assert_eq!(at(62., &frame), 3);
    assert_eq!(
        frame.inlay,
        Some(rect(GUTTER + 2. * CELL, 0., 3. * CELL, LINE))
    );
}

#[test]
fn rects_follow_the_inlay() {
    let frame = lay_out_inlay("ab[cd]", Some((2, "XYZ")));
    // The selection starts at the inlay's offset and ends past it.
    assert_eq!(frame.selections, [rect(GUTTER + 20., 0., 50., LINE)]);
    let frame = lay_out_inlay("abc|d", Some((2, "XYZ")));
    assert_eq!(frame.cursors, [rect(GUTTER + 60., 0., 2., LINE)]);
    let runs = &frame.rows[0].line.runs;
    let lens: Vec<_> = runs.iter().map(|run| (run.len, run.style)).collect();
    assert_eq!(lens, [(2, "text"), (3, "ghost"), (2, "text")]);
}

#[test]
fn other_rows_and_tabs_unchanged() {
    let plain = lay_out_inlay("ab\n|cd\nef", None);
    let with = lay_out_inlay("ab\n|cd\nef", Some((1, "XYZ")));
    assert_eq!(with.rows[1].line.text, plain.rows[1].line.text);
    assert_eq!(with.rows[2].line.text, plain.rows[2].line.text);
    assert_eq!(with.rows[0].line.text, "aXYZb");
    assert_eq!(with.cursors, plain.cursors);

    // The tab after the inlay keeps its width of 3.
    let frame = lay_out_inlay("|a\tb", Some((1, "XY")));
    assert_eq!(frame.rows[0].line.text, "aXY   b");
    assert_eq!(x_for(&frame, 2), 6. * f64::from(CELL));

    // An offset past the shown slice of a long row adds nothing.
    let long = format!("|{}", "x".repeat(MAX_SHAPED_ROW_BYTES + 100));
    let frame = lay_out_inlay(&long, Some((MAX_SHAPED_ROW_BYTES + 50, "XYZ")));
    assert!(!frame.rows[0].line.text.contains("XYZ"));
    assert_eq!(frame.inlay, None);
}
