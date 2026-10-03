//! Frame layout against a fake monospace measurer: every char is one 10 px
//! cell, CJK and emoji two. Rows are 20 px; the gutter is 70 px for files of
//! under 1000 rows.
//!
//! Notation: `|` is a caret, `[…]` a selection whose head is at its end, and
//! `{…}` one whose head is at its start. The first is primary.

use fe_buffer::Buffer;

use super::*;

pub(super) const CELL: f32 = 10.;
pub(super) const LINE: f32 = 20.;
pub(super) const GUTTER: f32 = 70.;
pub(super) const WIDTH: f32 = 400.;

pub(super) fn advance(ch: char) -> f32 {
    let wide = matches!(ch,
        '\u{1100}'..='\u{115F}' | '\u{2E80}'..='\u{A4CF}' | '\u{AC00}'..='\u{D7A3}'
        | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FF60}' | '\u{1F300}'..='\u{1FAFF}');
    if wide { 2. * CELL } else { CELL }
}

#[derive(Clone, Debug)]
pub(super) struct Line {
    pub(super) text: String,
    pub(super) runs: Vec<StyleRun<&'static str>>,
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.x_for_index(self.text.len())
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.text[..index].chars().map(advance).sum()
    }

    fn index_for_x(&self, x: f32) -> usize {
        let mut left = 0.;
        for (ix, ch) in self.text.char_indices() {
            if x < left + advance(ch) / 2. {
                return ix;
            }
            left += advance(ch);
        }
        self.text.len()
    }
}

pub(super) struct Mono;

impl CharWidth for Mono {
    fn char_width(&mut self, ch: char) -> f32 {
        advance(ch)
    }
}

impl TextMeasure for Mono {
    type Style = &'static str;
    type Line = Line;

    fn shape(&mut self, text: &str, runs: &[StyleRun<&'static str>]) -> Line {
        let covered: usize = runs.iter().map(|run| run.len).sum();
        assert_eq!(covered, text.len(), "runs must cover {text:?}");
        Line {
            text: text.to_string(),
            runs: runs.to_vec(),
        }
    }
}

pub(super) fn parse(marked: &str) -> (String, Vec<ResolvedSelection>) {
    let mut text = String::new();
    let mut selections = Vec::new();
    let mut open = None;
    for ch in marked.chars() {
        match ch {
            '|' => selections.push(ResolvedSelection {
                range: text.len()..text.len(),
                reversed: false,
            }),
            '[' | '{' => open = Some(text.len()),
            ']' | '}' => selections.push(ResolvedSelection {
                range: open.take().expect("balanced")..text.len(),
                reversed: ch == '}',
            }),
            _ => text.push(ch),
        }
    }
    (text, selections)
}

/// Lays out `marked` in a 400 × `height` editor scrolled to `scroll`.
fn lay_out(marked: &str, scroll: (f32, f32), autoscroll: bool, height: f32) -> Frame<Line> {
    lay_out_with(marked, scroll, autoscroll, height, |_| Vec::new())
}

fn lay_out_with(
    marked: &str,
    scroll: (f32, f32),
    autoscroll: bool,
    height: f32,
    highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<&'static str>>,
) -> Frame<Line> {
    lay_out_display(marked, scroll, autoscroll, height, |_| {}, highlights).0
}

/// Lays out `marked` with its rows wrapped or folded by `configure`.
fn lay_out_display(
    marked: &str,
    (x, y): (f32, f32),
    autoscroll: bool,
    height: f32,
    configure: impl FnOnce(&mut DisplayMap),
    highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<&'static str>>,
) -> (Frame<Line>, Text) {
    let (text, selections) = parse(marked);
    let buffer = Buffer::new(&text);
    let mut display = DisplayMap::new(buffer.current(), 4);
    configure(&mut display);
    let input = FrameInput {
        display: &display,
        tab_size: 4,
        selections: &selections,
        primary_head: selections[0].head(),
        scroll: Vec2 { x, y },
        autoscroll,
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
    let frame = layout_frame(
        &input,
        &mut Mono,
        &mut LongRows::default(),
        &mut Mono,
        highlights,
    );
    (frame, display.text().clone())
}

pub(super) fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// `rows` rows of `row N`, with a caret at the start of `caret_row`.
fn numbered_rows(rows: u32, caret_row: u32) -> String {
    (0..rows)
        .map(|row| {
            let caret = if row == caret_row { "|" } else { "" };
            format!("{caret}row {row}\n")
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn vertical_scroll() {
    // 100 rows in a 6-row editor; autoscroll keeps the caret 2 rows (40 px)
    // from either edge.
    // (caret row, scroll y, autoscroll) => (scroll y, rows laid out)
    let cases = [
        // Already in view at the top: clamps at 0.
        ((0, 0., true), (0., 0..6)),
        // Below the view: scrolls until it's 2 rows from the bottom.
        ((50, 0., true), (940., 47..53)),
        // Above the view: scrolls until it's 2 rows from the top.
        ((50, 2000., true), (960., 48..54)),
        // The last row can't keep a margin below it.
        ((99, 0., true), (1920., 96..100)),
        // Without autoscroll the caret may be off screen; only clamping applies.
        ((0, -50., false), (0., 0..6)),
        ((0, 30., false), (30., 1..8)),
        // Scrolling stops with the last row at the top.
        ((0, 5000., false), (1980., 99..100)),
    ];
    for ((caret_row, scroll_y, autoscroll), (want_scroll_y, want_rows)) in cases {
        let frame = lay_out(
            &numbered_rows(100, caret_row),
            (0., scroll_y),
            autoscroll,
            120.,
        );
        let rows: Vec<u32> = frame.rows.iter().map(|row| row.row).collect();
        let label = format!("caret row {caret_row}, scroll {scroll_y}");
        assert_eq!(frame.scroll.y, want_scroll_y, "{label}");
        assert_eq!(rows, want_rows.collect::<Vec<_>>(), "{label}");
        assert_eq!(frame.first_row, rows[0], "{label}");
    }
}

#[test]
fn horizontal_scroll() {
    // The text area is 330 px wide; autoscroll keeps the caret 4 em (40 px)
    // from either side, and scrolling stops 4 em past the widest row.
    let long = "x".repeat(100);
    let cases = [
        (format!("{}|{}", &long[..50], &long[50..]), 0., true, 210.),
        (format!("|{long}"), 300., true, 0.),
        (format!("|{long}"), 5000., false, 710.),
        (format!("|{long}"), -10., false, 0.),
        ("|ab".to_string(), 50., false, 0.),
    ];
    for (marked, scroll_x, autoscroll, want) in cases {
        let frame = lay_out(&marked, (scroll_x, 0.), autoscroll, 120.);
        assert_eq!(frame.scroll.x, want, "{marked:?} from {scroll_x}");
    }
}

#[test]
fn selections_and_cursors() {
    struct Case {
        marked: String,
        scroll_y: f32,
        selections: Vec<Rect>,
        cursors: Vec<Rect>,
        active_lines: Vec<Rect>,
    }
    let cases = [
        // A tab expands to the next 4-column stop and wide chars take two
        // cells. A selection reaching past a row's end also covers 0.6 em
        // for the line break.
        Case {
            marked: "\t[ab\n世]界x\nen|d".to_string(),
            scroll_y: 0.,
            selections: vec![
                rect(GUTTER + 40., 0., 26., LINE),
                rect(GUTTER, LINE, 20., LINE),
            ],
            cursors: vec![
                rect(GUTTER + 20., LINE, 2., LINE),
                rect(GUTTER + 20., 2. * LINE, 2., LINE),
            ],
            active_lines: vec![rect(0., 2. * LINE, WIDTH, LINE)],
        },
        // A reversed selection puts its cursor at the start.
        Case {
            marked: "a{bc}d".to_string(),
            scroll_y: 0.,
            selections: vec![rect(GUTTER + 10., 0., 20., LINE)],
            cursors: vec![rect(GUTTER + 10., 0., 2., LINE)],
            active_lines: vec![],
        },
        // Only the visible part of a selection is laid out, and a caret
        // scrolled out of view has no cursor or active line.
        Case {
            marked: format!("|r0\n[r1\nr2\nr3\nr]4\n{}", "r\n".repeat(20)),
            scroll_y: 2. * LINE,
            selections: vec![
                rect(GUTTER, 0., 26., LINE),
                rect(GUTTER, LINE, 26., LINE),
                rect(GUTTER, 2. * LINE, 10., LINE),
            ],
            cursors: vec![rect(GUTTER + 10., 2. * LINE, 2., LINE)],
            active_lines: vec![],
        },
    ];
    for case in cases {
        let frame = lay_out(&case.marked, (0., case.scroll_y), false, 120.);
        assert_eq!(frame.selections, case.selections, "{:?}", case.marked);
        assert_eq!(frame.cursors, case.cursors, "{:?}", case.marked);
        assert_eq!(frame.active_lines, case.active_lines, "{:?}", case.marked);
    }
}

#[test]
fn line_numbers_are_right_aligned_and_mark_caret_rows() {
    let frame = lay_out(&numbered_rows(12, 9), (0., 0.), false, 240.);
    let numbers: Vec<_> = frame
        .line_numbers
        .iter()
        .map(|(origin, line)| (line.text.as_str(), line.runs[0].style, origin.x, origin.y))
        .collect();
    // Numbers end 2 em left of the text.
    assert_eq!(numbers[0], ("1", "number", 40., 0.));
    assert_eq!(numbers[9], ("10", "active", 30., 9. * LINE));
    assert_eq!(numbers[11], ("12", "number", 30., 11. * LINE));

    // The gutter grows with the number of digits.
    let frame = lay_out(&numbered_rows(1000, 0), (0., 0.), false, 120.);
    assert_eq!(frame.text_left, GUTTER + CELL);
}

#[test]
fn rows_are_styled_by_their_spans() {
    // Row 1, `\tif "é"`, starts at offset 4; a comment runs into it from row 0.
    let span = |range, style| StyledSpan { range, style };
    let frame = lay_out_with("|abc\n\tif \"é\"", (0., 0.), false, 120., |range| {
        assert_eq!(range, 0..12);
        vec![
            span(1..5, "comment"),
            span(5..7, "keyword"),
            span(8..12, "string"),
        ]
    });
    let runs = |row: usize| -> Vec<_> {
        let line = &frame.rows[row].line;
        line.runs.iter().map(|run| (run.len, run.style)).collect()
    };
    assert_eq!(runs(0), [(1, "text"), (2, "comment")]);
    assert_eq!(
        runs(1),
        [(4, "comment"), (2, "keyword"), (1, "text"), (4, "string")]
    );
}

#[test]
fn hit_testing() {
    let (frame, text) = lay_out_display(
        "|\tab\n世界x\nend",
        (0., 0.),
        false,
        120.,
        |_| {},
        |_| Vec::new(),
    );
    let cases = [
        // Inside an expanded tab, positions snap to before it.
        ((GUTTER + 26., 5.), 0),
        ((GUTTER + 38., 5.), 1),
        // The closest boundary of a wide char.
        ((GUTTER + 25., LINE + 5.), 7),
        ((GUTTER + 35., LINE + 5.), 10),
        // In the gutter: the row's start. Past its end: the row's end.
        ((0., 2. * LINE + 5.), 12),
        ((WIDTH, 5.), 3),
        // Above or below the text.
        ((GUTTER, -30.), 0),
        ((GUTTER, 10. * LINE), text.len()),
    ];
    for ((x, y), want) in cases {
        assert_eq!(
            frame.offset_at(&text, Vec2 { x, y }, &mut LongRows::default(), &mut Mono),
            want,
            "at ({x}, {y})"
        );
    }
    assert_eq!(
        frame.caret_bounds(7, &mut LongRows::default(), &mut Mono),
        Some(rect(GUTTER + 20., LINE, 2., LINE))
    );
}

#[test]
fn wrapped_rows() {
    // Wrapped at 6 cells: `aaaa `, `bbbb `, `cc`, then `d`.
    let wrap = |display: &mut DisplayMap| {
        display.set_wrap_width(Some(6.), &mut crate::Monospace);
        display.wrap_pending(0, usize::MAX, &mut crate::Monospace);
    };
    let (frame, text) = lay_out_display("aa[aa bbbb c]c\nd", (500., 0.), true, 120., wrap, |_| {
        Vec::new()
    });
    let rows: Vec<_> = frame
        .rows
        .iter()
        .map(|row| (row.row, row.buffer_row, row.start..row.end, row.last))
        .collect();
    assert_eq!(
        rows,
        [
            (0, 0, 0..5, false),
            (1, 0, 5..10, false),
            (2, 0, 10..12, true),
            (3, 1, 13..14, true),
        ]
    );
    // Wrapped rows don't scroll sideways, and only first rows are numbered.
    assert_eq!(frame.scroll.x, 0.);
    let numbered: Vec<f32> = frame.line_numbers.iter().map(|(at, _)| at.y).collect();
    assert_eq!(numbered, [0., 3. * LINE]);
    // A selection across soft wraps marks no line break until the real one.
    assert_eq!(
        frame.selections,
        [
            rect(GUTTER + 20., 0., 30., LINE),
            rect(GUTTER, LINE, 50., LINE),
            rect(GUTTER, 2. * LINE, 10., LINE),
        ]
    );
    // The caret at a wrap point is at the start of the next row.
    assert_eq!(
        frame.caret_bounds(5, &mut LongRows::default(), &mut Mono),
        Some(rect(GUTTER, LINE, 2., LINE))
    );
    // Clicking past a wrapped row's end stays on that row.
    assert_eq!(
        frame.offset_at(
            &text,
            Vec2 { x: WIDTH, y: 5. },
            &mut LongRows::default(),
            &mut Mono
        ),
        4
    );
    assert_eq!(
        frame.offset_at(
            &text,
            Vec2 {
                x: WIDTH,
                y: 2. * LINE + 5.
            },
            &mut LongRows::default(),
            &mut Mono,
        ),
        12
    );
}

#[test]
fn folded_rows() {
    let fold = |display: &mut DisplayMap| {
        display.fold(0..=2);
    };
    let (frame, _) = lay_out_display(
        "def [a():\n    one\n    two\nend]",
        (0., 0.),
        false,
        120.,
        fold,
        |_| Vec::new(),
    );
    let rows: Vec<_> = frame
        .rows
        .iter()
        .map(|row| (row.row, row.buffer_row))
        .collect();
    assert_eq!(rows, [(0, 0), (1, 3)]);
    let numbers: Vec<_> = frame
        .line_numbers
        .iter()
        .map(|(_, line)| line.text.as_str())
        .collect();
    assert_eq!(numbers, ["1", "4"]);
    // The marker sits a cell after the header's text.
    let markers: Vec<_> = frame
        .fold_markers
        .iter()
        .map(|(at, line)| (at.x, at.y, line.text.as_str()))
        .collect();
    assert_eq!(markers, [(GUTTER + 90., 0., FOLD_MARKER)]);
    // The selection covers the header's line break, which hides the folded
    // rows, then the row after them.
    assert_eq!(
        frame.selections,
        [
            rect(GUTTER + 40., 0., 46., LINE),
            rect(GUTTER, LINE, 30., LINE)
        ]
    );
    assert_eq!(frame.cursors, [rect(GUTTER + 30., LINE, 2., LINE)]);
}
