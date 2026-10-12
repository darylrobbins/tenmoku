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

/// A long row of 9000 `x`, with `inlay` at offset 40, laid out at `scroll_x`
/// with the caret at `head`; `long_rows` is the frame's, to map with.
fn lay_out_long_inlay_with<M: TextMeasure<Style = &'static str>>(
    measure: &mut M,
    inlay: (usize, &str),
    head: usize,
    scroll_x: f32,
    autoscroll: bool,
    long_rows: &mut LongRows,
) -> (Frame<M::Line>, Buffer) {
    let buffer = Buffer::new(&"x".repeat(9000));
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: head..head,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: head,
        scroll: Vec2 { x: scroll_x, y: 0. },
        autoscroll,
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
    let inlay = Inlay {
        offset: inlay.0,
        text: inlay.1.to_string(),
        style: "ghost",
    };
    let frame = layout_frame_with_inlay(
        &input,
        &BlockMap::default(),
        Some(&inlay),
        measure,
        long_rows,
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    (frame, buffer)
}

fn lay_out_long_inlay(
    head: usize,
    scroll_x: f32,
    autoscroll: bool,
    long_rows: &mut LongRows,
) -> (Frame<Line>, Buffer) {
    lay_out_long_inlay_with(
        &mut Mono,
        (40, "XYZ"),
        head,
        scroll_x,
        autoscroll,
        long_rows,
    )
}

#[test]
fn long_row_mappings_follow_the_inlay_past_the_slice() {
    let mut long_rows = LongRows::default();
    let (frame, buffer) = lay_out_long_inlay(0, 0., false, &mut long_rows);
    let row = &frame.rows[0];
    assert!(row.line.text.contains("XYZ"));
    let end = row.shown().end;
    assert!(end < 9000 - 60);
    let cell = f64::from(CELL);
    let mut x_at = |offset: usize| row.x_for_offset(offset, &mut long_rows, &mut Mono);
    // Positions keep moving right across the slice's right boundary.
    assert_eq!(x_at(end), (end + 3) as f64 * cell);
    assert_eq!(x_at(end + 1), (end + 4) as f64 * cell);
    assert_eq!(x_at(end + 50), (end + 53) as f64 * cell);

    // A click past the slice finds the offset, not one three characters on.
    let mut at = |offset: usize| {
        frame.offset_at(
            buffer.current().text(),
            Vec2 {
                x: GUTTER + (offset + 3) as f32 * CELL + 2.,
                y: 5.,
            },
            &mut long_rows,
            &mut Mono,
        )
    };
    assert_eq!(at(end + 50), end + 50);
    assert_eq!(at(end + 1), end + 1);
}

#[test]
fn long_row_autoscroll_counts_the_inlay_before_the_caret() {
    let mut long_rows = LongRows::default();
    let (frame, _) = lay_out_long_inlay(60, 0., true, &mut long_rows);
    // The caret is at x 630 with the inlay, 4 ems from the right edge.
    let text_width = WIDTH - GUTTER;
    assert_eq!(frame.scroll.x, 630. - text_width + 4. * CELL);
    let caret = frame.cursors[0];
    assert!(caret.x <= WIDTH - 4. * CELL + 0.5, "{caret:?}");

    // An inlay too far back to shape with the slice adds nothing: the caret is far right.
    let mut long_rows = LongRows::default();
    let (frame, _) = lay_out_long_inlay(8900, 0., true, &mut long_rows);
    assert!(!frame.rows[0].line.text.contains("XYZ"));
    assert_eq!(frame.scroll.x, 89000. - text_width + 4. * CELL);
}

#[test]
fn long_row_autoscroll_shows_the_caret_past_a_wide_inlay() {
    // The inlay is wider than the view: the caret is either scrolled past
    // it, or the inlay is scrolled away and the caret is where it is
    // without; either way it's on screen.
    let wide = "W".repeat(100);
    let mut long_rows = LongRows::default();
    let (frame, _) = lay_out_long_inlay_with(&mut Mono, (40, &wide), 60, 0., true, &mut long_rows);
    let caret = frame.cursors.first().expect("the caret is laid out");
    assert!(caret.x >= GUTTER, "{caret:?}");
    assert!(caret.x + CURSOR_WIDTH <= WIDTH, "{caret:?}");
}

#[test]
fn long_row_scroll_limit_counts_an_inlay_the_clamp_brings_into_the_slice() {
    // An excessive scroll, with the inlay near the row's end: the slice
    // proposed for it leaves the inlay out, the clamped one holds it, and
    // the furthest scroll allows for its width.
    let mut long_rows = LongRows::default();
    let (frame, _) =
        lay_out_long_inlay_with(&mut Mono, (8990, "XYZ"), 0, 1e9, false, &mut long_rows);
    let row = &frame.rows[0];
    assert!(row.line.text.contains("XYZ"));
    let text_width = WIDTH - GUTTER;
    assert_eq!(frame.scroll.x, 90030. - text_width + 4. * CELL);
}

/// Draws "X" after an "x" half as wide: text measured alone is wider than
/// the same text in a row.
struct Ctx;

struct CtxLine(Line);

fn ctx_advance(text: &str, ix: usize, ch: char) -> f32 {
    if ch == 'X' && text[..ix].ends_with('x') {
        CELL / 2.
    } else {
        advance(ch)
    }
}

impl MeasuredLine for CtxLine {
    fn width(&self) -> f32 {
        self.x_for_index(self.0.text.len())
    }

    fn x_for_index(&self, index: usize) -> f32 {
        let text = &self.0.text;
        text[..index]
            .char_indices()
            .map(|(ix, ch)| ctx_advance(text, ix, ch))
            .sum()
    }

    fn index_for_x(&self, x: f32) -> usize {
        let text = &self.0.text;
        let mut left = 0.;
        for (ix, ch) in text.char_indices() {
            let width = ctx_advance(text, ix, ch);
            if x < left + width / 2. {
                return ix;
            }
            left += width;
        }
        text.len()
    }
}

impl TextMeasure for Ctx {
    type Style = &'static str;
    type Line = CtxLine;

    fn shape(&mut self, text: &str, runs: &[StyleRun<&'static str>]) -> CtxLine {
        CtxLine(Mono.shape(text, runs))
    }
}

#[test]
fn long_row_autoscroll_uses_the_inlay_as_measured_in_the_row() {
    // "XYZ" is 30 wide alone but 25 after an "x", as it is in the row.
    let mut long_rows = LongRows::default();
    let (frame, _) = lay_out_long_inlay_with(&mut Ctx, (40, "XYZ"), 60, 0., true, &mut long_rows);
    let row = &frame.rows[0];
    assert_eq!(row.inlay_width, 25.);
    // The caret is at x 625 with the inlay, 4 ems from the right edge.
    let text_width = WIDTH - GUTTER;
    assert_eq!(frame.scroll.x, 625. - text_width + 4. * CELL);
}

#[test]
fn long_row_autoscroll_shows_the_caret_past_an_inlay_wider_than_a_slice() {
    // The inlay is wider than `MAX_SHAPED_ROW_BYTES` characters, so the
    // view past it is in the index at the row's start, where the inlay
    // is still in the slice.
    let wide = "W".repeat(8200);
    let mut long_rows = LongRows::default();
    let (frame, _) = lay_out_long_inlay_with(&mut Mono, (40, &wide), 60, 0., true, &mut long_rows);
    let caret = frame.cursors.first().expect("the caret is laid out");
    assert!(caret.x >= GUTTER, "{caret:?}");
    assert!(caret.x + CURSOR_WIDTH <= WIDTH, "{caret:?}");
}

/// Draws every "W" twice as wide as it measures alone.
struct Wide;

struct WideLine(Line);

fn wide_advance(ch: char) -> f32 {
    if ch == 'W' {
        2. * advance(ch)
    } else {
        advance(ch)
    }
}

impl MeasuredLine for WideLine {
    fn width(&self) -> f32 {
        self.x_for_index(self.0.text.len())
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.0.text[..index].chars().map(wide_advance).sum()
    }

    fn index_for_x(&self, x: f32) -> usize {
        let mut left = 0.;
        for (ix, ch) in self.0.text.char_indices() {
            let width = wide_advance(ch);
            if x < left + width / 2. {
                return ix;
            }
            left += width;
        }
        self.0.text.len()
    }
}

impl TextMeasure for Wide {
    type Style = &'static str;
    type Line = WideLine;

    fn shape(&mut self, text: &str, runs: &[StyleRun<&'static str>]) -> WideLine {
        WideLine(Mono.shape(text, runs))
    }
}

#[test]
fn long_row_autoscroll_shows_the_caret_past_an_inlay_shaped_wider_than_measured() {
    // 8200 "W" measure 10 each but are shaped at 20: the displacement of
    // the text after the inlay is the shaped width, or the slice for a
    // view past it misses the inlay.
    let wide = "W".repeat(8200);
    let buffer = Buffer::new(&"x".repeat(30_000));
    let display = DisplayMap::new(buffer.current(), 4);
    let selections = [ResolvedSelection {
        range: 60..60,
        reversed: false,
    }];
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: 60,
        scroll: Vec2::default(),
        autoscroll: true,
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
    let inlay = Inlay {
        offset: 40,
        text: wide,
        style: "ghost",
    };
    let frame = layout_frame_with_inlay(
        &input,
        &BlockMap::default(),
        Some(&inlay),
        &mut Wide,
        &mut LongRows::default(),
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    );
    let caret = frame.cursors.first().expect("the caret is laid out");
    assert!(caret.x >= GUTTER, "{caret:?}");
    assert!(caret.x + CURSOR_WIDTH <= WIDTH, "{caret:?}");
}
