//! `RowLayout::xs_for_offsets` gives what `RowLayout::x_for_offset` gives
//! for each offset: on short rows with tabs, hints and an inlay, and on long
//! rows on both sides of the shaped slice, with hints outside it.
//!
//! Every char is a cell 10 px wide; rows are 20 px.

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{
    BlockMap, CharWidth, DisplayMap, LongRows,
    layout::{
        Frame, FrameInput, FrameStyles, Inlay, MeasuredLine, StyleRun, TextMeasure, Vec2,
        layout_frame_with_hints,
    },
};

const CELL: f32 = 10.;

#[derive(Clone, Debug, PartialEq)]
struct Line(String);

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.0.chars().count() as f32 * CELL
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.0[..index.min(self.0.len())].chars().count() as f32 * CELL
    }

    fn index_for_x(&self, x: f32) -> usize {
        let column = (x / CELL).round().max(0.) as usize;
        (self.0.char_indices().nth(column)).map_or(self.0.len(), |(ix, _)| ix)
    }
}

struct Mono;

impl CharWidth for Mono {
    fn char_width(&mut self, _: char) -> f32 {
        CELL
    }
}

impl TextMeasure for Mono {
    type Style = ();
    type Line = Line;

    fn shape(&mut self, text: &str, _: &[StyleRun<()>]) -> Line {
        Line(text.to_string())
    }
}

/// A frame of `text`, scrolled `scroll_x` across, with `hints` and `inlay`.
fn frame(
    display: &DisplayMap,
    scroll_x: f32,
    hints: &[(usize, &str)],
    inlay: Option<(usize, &str)>,
    long_rows: &mut LongRows,
) -> Frame<Line> {
    let selections = [ResolvedSelection {
        range: 0..0,
        reversed: false,
    }];
    let input = FrameInput {
        display,
        tab_size: 4,
        selections: &selections,
        primary_head: 0,
        scroll: Vec2 { x: scroll_x, y: 0. },
        autoscroll: false,
        width: 800.,
        height: 200.,
        line_height: 20.,
        em: CELL,
        styles: FrameStyles {
            text: (),
            line_number: (),
            active_line_number: (),
        },
    };
    let to_inlay = |(offset, text): (usize, &str)| Inlay {
        offset,
        text: text.to_string(),
        style: (),
    };
    let hints: Vec<_> = hints.iter().copied().map(to_inlay).collect();
    let inlay = inlay.map(to_inlay);
    layout_frame_with_hints(
        &input,
        &BlockMap::default(),
        inlay.as_ref(),
        &hints,
        &mut Mono,
        long_rows,
        &mut Mono,
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

/// Every char boundary of `text` in `range`.
fn boundaries(text: &str, range: Range<usize>) -> Vec<usize> {
    range
        .filter(|&offset| text.is_char_boundary(offset))
        .collect()
}

fn check(frame: &Frame<Line>, text: &str, long_rows: &mut LongRows) {
    for row in &frame.rows {
        let offsets = boundaries(text, row.start..row.end + 1);
        let mut at_once = Vec::new();
        row.xs_for_offsets(&offsets, long_rows, &mut Mono, &mut at_once);
        assert_eq!(at_once.len(), offsets.len());
        for (offset, x) in offsets.iter().zip(&at_once) {
            let one = row.x_for_offset(*offset, long_rows, &mut Mono);
            assert!(
                (one - x).abs() <= 1e-6 * one.abs().max(1.),
                "row {} offset {offset}: {one} one at a time, {x} at once",
                row.row
            );
        }
    }
}

#[test]
fn short_rows_with_tabs_hints_and_an_inlay() {
    let text = "\tlet\tx = é;\n\t\tfoo(a, b)\nplain 😀 row\n";
    let display = DisplayMap::new(Buffer::new(text).current(), 4);
    let hints = [(5, ": u8"), (16, "a: "), (19, "b: ")];
    let mut long_rows = LongRows::default();
    let frame = frame(&display, 0., &hints, Some((8, "ghost")), &mut long_rows);
    check(&frame, text, &mut long_rows);
}

#[test]
fn long_rows_on_both_sides_of_the_slice() {
    let row = "ab\tcé中 😀xyz ".repeat(3000);
    let text = format!("short\n{row}\nend");
    let display = DisplayMap::new(Buffer::new(&text).current(), 4);
    let start = 6;
    // Hints before, inside and after the view's slice, scrolled midway.
    let hints = [
        (start + 40, "h1"),
        (start + 20_000, "h2: x"),
        (start + 30_000, "h3"),
    ];
    let mut long_rows = LongRows::default();
    let frame = frame(
        &display,
        100_000.,
        &hints,
        Some((start + 25_000, "in")),
        &mut long_rows,
    );
    let long = frame.rows.iter().find(|row| row.slice.is_some());
    assert!(long.is_some(), "the long row is sliced");
    check(&frame, &text, &mut long_rows);
}
