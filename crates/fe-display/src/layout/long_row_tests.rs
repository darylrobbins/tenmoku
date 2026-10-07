//! Rows longer than [`MAX_SHAPED_ROW_BYTES`], against the fake measurer of
//! the layout tests: with soft wrap off, only a slice around the view is
//! shaped, and positions elsewhere in the row come from its index.

use fe_buffer::Buffer;

use super::{tests::*, *};

const TEXT_WIDTH: f32 = WIDTH - GUTTER;

/// [`Mono`], counting the bytes shaped.
#[derive(Default)]
struct Counting {
    bytes: usize,
}

impl TextMeasure for Counting {
    type Style = &'static str;
    type Line = Line;

    fn shape(&mut self, text: &str, runs: &[StyleRun<&'static str>]) -> Line {
        self.bytes += text.len();
        Mono.shape(text, runs)
    }
}

/// [`Mono`]'s char widths, counting the chars measured.
#[derive(Default)]
struct CountingWidths {
    calls: usize,
}

impl CharWidth for CountingWidths {
    fn char_width(&mut self, ch: char) -> f32 {
        self.calls += 1;
        advance(ch)
    }
}

/// An editor's display and long rows, kept from frame to frame as the UI
/// keeps them.
struct Editor {
    text: String,
    display: DisplayMap,
    long_rows: LongRows,
    height: f32,
}

impl Editor {
    fn new(text: String) -> Self {
        let display = DisplayMap::new(Buffer::new(&text).current(), 4);
        Self {
            text,
            display,
            long_rows: LongRows::default(),
            height: 120.,
        }
    }

    /// Lays out a frame 400 wide with selections from and to each pair of
    /// `selections`, the first primary.
    fn lay_out(
        &mut self,
        selections: &[(usize, usize)],
        scroll_x: f32,
        autoscroll: bool,
        measure: &mut impl TextMeasure<Style = &'static str, Line = Line>,
        highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<&'static str>>,
    ) -> Frame<Line> {
        self.lay_out_measuring(
            selections, scroll_x, autoscroll, measure, &mut Mono, highlights,
        )
    }

    fn lay_out_measuring(
        &mut self,
        selections: &[(usize, usize)],
        scroll_x: f32,
        autoscroll: bool,
        measure: &mut impl TextMeasure<Style = &'static str, Line = Line>,
        widths: &mut dyn CharWidth,
        highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<&'static str>>,
    ) -> Frame<Line> {
        let selections: Vec<_> = selections
            .iter()
            .map(|&(start, end)| ResolvedSelection {
                range: start..end,
                reversed: false,
            })
            .collect();
        let input = FrameInput {
            display: &self.display,
            tab_size: 4,
            selections: &selections,
            primary_head: selections[0].head(),
            scroll: Vec2 { x: scroll_x, y: 0. },
            autoscroll,
            width: WIDTH,
            height: self.height,
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
            measure,
            &mut self.long_rows,
            widths,
            highlights,
            |_| Vec::new(),
        )
    }

    fn offset_at(&mut self, frame: &Frame<Line>, x: f32) -> usize {
        let text = self.display.text();
        let position = Vec2 { x, y: LINE / 4. };
        frame.offset_at(text, position, &mut self.long_rows, &mut Mono)
    }

    fn caret_x(&mut self, frame: &Frame<Line>, offset: usize) -> f32 {
        let caret = frame.caret_bounds(offset, &mut self.long_rows, &mut Mono);
        caret.expect("on screen").x
    }
}

#[test]
fn shapes_only_the_visible_slice() {
    let mut editor = Editor::new("abcdefgh".repeat(10 << 17));
    let row_width = editor.text.len() as f32 * CELL;
    // The view, a margin either side, a char past each end and a line number.
    let most = ((TEXT_WIDTH + 2. * SLICE_MARGIN_EMS * CELL) / CELL) as usize + 3;
    for x in [0., row_width / 2., row_width] {
        let mut measure = Counting::default();
        let frame = editor.lay_out(&[(0, 0)], x, false, &mut measure, |_| Vec::new());
        assert!(
            measure.bytes <= most,
            "{} bytes shaped at {x}",
            measure.bytes
        );

        let row = &frame.rows[0];
        let view = f64::from(frame.scroll.x)..f64::from(frame.scroll.x + TEXT_WIDTH);
        let shaped = row.start_x()..row.start_x() + f64::from(row.line.width());
        assert!(shaped.start <= view.start, "{shaped:?} covers {view:?}");
        assert!(
            shaped.end >= view.end || row.shown().end == row.end,
            "{shaped:?} covers {view:?}"
        );
        assert_eq!(frame.line_origin(row).x, frame.view_x(row.start_x()));
    }
    // Scrolling reaches 4 em past the row's end.
    let frame = editor.lay_out(&[(0, 0)], f32::MAX, false, &mut Mono, |_| Vec::new());
    assert_eq!(frame.scroll.x, row_width - TEXT_WIDTH + 4. * CELL);
}

#[test]
fn carets_selections_and_clicks_far_right() {
    // 50 px per pattern: `中` is two cells.
    let pattern = "abc中";
    let mut editor = Editor::new(pattern.repeat((10 << 20) / pattern.len()));
    let len = editor.text.len();
    let mid = 6 * 833_334;
    let frame = editor.lay_out(&[(mid, mid)], 0., true, &mut Mono, |_| Vec::new());
    let caret = frame.cursors[0];
    // That far along, scroll positions are multiples of 4 px, an `f32`'s
    // precision, so the caret may be up to 2 px past the margin.
    let in_margins = GUTTER + 4. * CELL - 2.0..=WIDTH - 4. * CELL + 2.;
    assert!(in_margins.contains(&caret.x), "{caret:?}");
    assert_eq!(
        frame.caret_bounds(mid, &mut editor.long_rows, &mut Mono),
        Some(caret)
    );

    // Each char boundary around the caret round-trips through a click.
    for offset in [
        mid - 6,
        mid - 5,
        mid - 4,
        mid - 3,
        mid,
        mid + 1,
        mid + 2,
        mid + 3,
        mid + 6,
    ] {
        let x = editor.caret_x(&frame, offset);
        assert_eq!(x, caret.x + (x - caret.x).round(), "x is exact");
        assert_eq!(editor.offset_at(&frame, x + 1.), offset, "click at {x}");
    }
    // Clicks closest to either side of `中`.
    assert_eq!(editor.offset_at(&frame, caret.x - 11.), mid - 3);
    assert_eq!(editor.offset_at(&frame, caret.x - 9.), mid);

    // Far outside the slice, the index places carets and clicks.
    for x in [-1e5, 1e5] {
        let offset = editor.offset_at(&frame, caret.x + x);
        assert!(editor.text.is_char_boundary(offset));
        let back = editor.caret_x(&frame, offset);
        assert!((back - (caret.x + x)).abs() <= CELL, "{x}: at {back}");
    }
    assert_eq!(editor.caret_x(&frame, 12), frame.view_x(100.));

    // A selection ending at the caret, and one from the row's start, which
    // reaches off screen.
    let frame = editor.lay_out(
        &[(mid - 10, mid), (0, mid)],
        frame.scroll.x,
        false,
        &mut Mono,
        |_| Vec::new(),
    );
    let left = editor.caret_x(&frame, mid - 10);
    assert_eq!(caret.x - left, 80.);
    assert_eq!(
        frame.selections,
        [
            rect(left, 0., 80., LINE),
            rect(GUTTER - CELL, 0., caret.x - GUTTER + CELL, LINE),
        ]
    );

    // Autoscroll brings the row's end into view. Past 2^26 px, scroll
    // positions are multiples of 8 px, so the caret may be up to 4 px past
    // the margin.
    let frame = editor.lay_out(&[(len, len)], 0., true, &mut Mono, |_| Vec::new());
    let in_margins = GUTTER + 4. * CELL - 4.0..=WIDTH - 4. * CELL + 4.;
    assert!(in_margins.contains(&frame.cursors[0].x));
    assert_eq!(editor.caret_x(&frame, len), frame.cursors[0].x);
    assert_eq!(editor.offset_at(&frame, WIDTH), len);
}

#[test]
fn highlights_query_the_visible_slice() {
    let long = "ab\tcé中 ".repeat(2000);
    let mut editor = Editor::new(format!("a\n{long}\nb"));
    let row_start = 2;
    let b_start = row_start + long.len() + 1;
    let style_of = |ch: char| match ch {
        'é' => "accent",
        '中' => "wide",
        _ => "text",
    };
    let text = editor.text.clone();
    let mut queries = Vec::new();
    let frame = editor.lay_out(&[(0, 0)], 50_000., false, &mut Mono, |range| {
        queries.push(range.clone());
        text[range.clone()]
            .char_indices()
            .filter(|&(_, ch)| style_of(ch) != "text")
            .map(|(ix, ch)| StyledSpan {
                range: range.start + ix..range.start + ix + ch.len_utf8(),
                style: style_of(ch),
            })
            .collect()
    });

    let row = &frame.rows[1];
    let shown = row.shown();
    assert!(row_start < shown.start && shown.len() < 200, "{shown:?}");
    assert_eq!(queries, [0..1, b_start..b_start + 1, shown.clone()]);

    // The slice expands to the same tab stops as the whole row.
    let whole = ExpandedRow::new(&long, 4);
    let display = |offset: usize| whole.to_display(offset - row_start);
    assert_eq!(
        row.expanded.text(),
        &whole.text()[display(shown.start)..display(shown.end)]
    );
    // Each byte is drawn in its char's style.
    let mut at = 0;
    for run in &row.line.runs {
        for (ix, _) in row.expanded.text()[at..at + run.len].char_indices() {
            let display = at + ix;
            let offset = shown.start + row.expanded.to_buffer(display, Bias::Left);
            let ch = text[offset..].chars().next().unwrap();
            assert_eq!(run.style, style_of(ch), "{ch:?} at {offset}");
        }
        at += run.len;
    }
    assert_eq!(at, row.expanded.text().len());
}

#[test]
fn every_long_row_on_screen_keeps_its_index() {
    const ROWS: usize = 150;
    let row = "x".repeat(MAX_SHAPED_ROW_BYTES + 10);
    let mut editor = Editor::new(vec![row.as_str(); ROWS].join("\n"));
    editor.height = ROWS as f32 * LINE;
    let row_width = row.len() as f64 * f64::from(CELL);
    for frame in 0..2 {
        let widths = &mut CountingWidths::default();
        let laid_out =
            editor.lay_out_measuring(&[(0, 0)], 0., false, &mut Mono, widths, |_| Vec::new());
        assert_eq!(laid_out.rows.len(), ROWS);
        if frame > 0 {
            assert_eq!(widths.calls, 0, "indexes were rebuilt");
        }
        // Each row's end is far right of its slice.
        for row in &laid_out.rows {
            let caret = laid_out.caret_bounds(row.end, &mut editor.long_rows, &mut Mono);
            assert_eq!(caret.unwrap().x, laid_out.view_x(row_width));
        }
    }
}

#[test]
fn rows_waiting_to_be_wrapped_are_cut_off() {
    let long = "x".repeat(MAX_SHAPED_ROW_BYTES + 100);
    let mut editor = Editor::new(format!("a\nb\n{long}\nc"));
    // Wrapping is pending until the frame loop gets to it.
    editor
        .display
        .set_wrap_width(Some(100.), &mut crate::Monospace);
    let mut queries = Vec::new();
    let frame = editor.lay_out(&[(0, 0)], 0., false, &mut Mono, |range| {
        queries.push(range);
        Vec::new()
    });
    let long_start = 4;
    let c_start = long_start + long.len() + 1;
    assert_eq!(
        queries,
        [0..long_start + MAX_SHAPED_ROW_BYTES, c_start..c_start + 1]
    );
    assert_eq!(frame.rows[2].line.text.len(), MAX_SHAPED_ROW_BYTES);
    assert_eq!(
        frame.rows[2].shown(),
        long_start..long_start + MAX_SHAPED_ROW_BYTES
    );
}
