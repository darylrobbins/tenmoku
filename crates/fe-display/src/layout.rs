//! Lays out one frame of an editor: scrolling, the visible display rows and
//! their style runs, selection and cursor rectangles, line numbers and fold
//! markers.
//!
//! This is plain arithmetic over a [`TextMeasure`], so it runs without a
//! window. The UI implements the trait with its text shaper, then paints what
//! [`layout_frame`] returns; tests use a fake monospace measurer.
//!
//! Positions are in pixels from the editor's top-left corner.

use std::ops::Range;

use fe_buffer::ResolvedSelection;
use fe_text::{Bias, Text};

use crate::{DisplayMap, ExpandedRow};

/// Longest prefix of a row that is shaped and painted. Rows beyond this
/// (minified files, data dumps) are cut off rather than stalling a frame.
pub const MAX_SHAPED_ROW_BYTES: usize = 8 * 1024;

/// Shown after a row whose following rows are folded.
pub const FOLD_MARKER: &str = "⋯";

/// A point, or an offset such as the scroll position, in pixels.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn from_corners(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }
}

/// `len` bytes of displayed text drawn in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleRun<S> {
    pub len: usize,
    pub style: S,
}

/// A buffer range to draw in a style, such as a highlighted token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan<S> {
    pub range: Range<usize>,
    pub style: S,
}

/// Shapes text into lines that can be measured. The UI implements this with
/// its text system.
pub trait TextMeasure {
    /// How a run is drawn, such as its colour.
    type Style: Clone;
    type Line: MeasuredLine;

    /// Shapes one row of displayed text (tabs already expanded, no newline).
    /// `runs` cover `text` exactly.
    fn shape(&mut self, text: &str, runs: &[StyleRun<Self::Style>]) -> Self::Line;
}

/// A shaped line. Indices are byte offsets into its text.
pub trait MeasuredLine {
    fn width(&self) -> f32;
    /// The x of the boundary before `index`, from the line's start.
    fn x_for_index(&self, index: usize) -> f32;
    /// The boundary closest to `x`.
    fn index_for_x(&self, x: f32) -> usize;
}

/// Styles that aren't syntax highlights.
#[derive(Clone, Debug)]
pub struct FrameStyles<S> {
    pub text: S,
    /// Line numbers and fold markers.
    pub line_number: S,
    /// Line numbers of rows with a caret.
    pub active_line_number: S,
}

/// Everything a frame is laid out from.
pub struct FrameInput<'a, S> {
    /// The rows to show, synced with the buffer.
    pub display: &'a DisplayMap,
    pub tab_size: usize,
    pub selections: &'a [ResolvedSelection],
    /// The primary cursor, which autoscroll keeps in view.
    pub primary_head: usize,
    /// The scroll position before this frame; the frame clamps it.
    pub scroll: Vec2,
    /// Scroll the primary cursor into view.
    pub autoscroll: bool,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    /// The font's em advance, which sizes the gutter and scroll margins.
    pub em: f32,
    pub styles: FrameStyles<S>,
}

/// One frame's layout, kept after painting for hit testing and IME.
#[derive(Clone, Debug)]
pub struct Frame<L> {
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    /// The clamped scroll position (both ≥ 0).
    pub scroll: Vec2,
    /// X of column 0 of the text, before horizontal scrolling. Everything
    /// left of it is gutter.
    pub text_left: f32,
    /// Display rows in the document.
    pub row_count: u32,
    pub first_row: u32,
    pub rows: Vec<RowLayout<L>>,
    /// Full-width bands behind rows that have a caret.
    pub active_lines: Vec<Rect>,
    pub selections: Vec<Rect>,
    pub cursors: Vec<Rect>,
    pub line_numbers: Vec<(Vec2, L)>,
    /// [`FOLD_MARKER`]s after rows with folded rows below.
    pub fold_markers: Vec<(Vec2, L)>,
}

/// A display row: all of a buffer row or, soft wrapped, part of it.
#[derive(Clone, Debug)]
pub struct RowLayout<L> {
    /// The display row.
    pub row: u32,
    pub buffer_row: u32,
    /// The buffer offsets the row covers. Only the first
    /// [`MAX_SHAPED_ROW_BYTES`] are shaped.
    pub start: usize,
    pub end: usize,
    /// Ends its buffer row, rather than at a soft wrap.
    pub last: bool,
    pub expanded: ExpandedRow,
    pub line: L,
}

impl<L: MeasuredLine> RowLayout<L> {
    /// X offset (from column 0) of a buffer offset in this row.
    pub fn x_for_offset(&self, offset: usize) -> f32 {
        let display = self.expanded.to_display(offset.saturating_sub(self.start));
        self.line
            .x_for_index(display.min(self.expanded.text().len()))
    }

    /// Whether the caret at `offset` is on this row. A soft wrap point
    /// starts the row after it.
    pub fn contains(&self, offset: usize) -> bool {
        self.start <= offset && (offset < self.end || offset == self.end && self.last)
    }
}

impl<L: MeasuredLine> Frame<L> {
    pub fn row(&self, row: u32) -> Option<&RowLayout<L>> {
        self.rows.get(row.checked_sub(self.first_row)? as usize)
    }

    /// The row in this frame with the caret at `offset`.
    pub fn row_containing(&self, offset: usize) -> Option<&RowLayout<L>> {
        self.rows.iter().find(|row| row.contains(offset))
    }

    /// Rows that fit entirely.
    pub fn visible_rows(&self) -> u32 {
        (self.height / self.line_height).floor() as u32
    }

    pub fn row_top(&self, row: u32) -> f32 {
        self.line_height * row as f32 - self.scroll.y
    }

    /// Where a row's line is painted.
    pub fn row_origin(&self, row: u32) -> Vec2 {
        Vec2 {
            x: self.text_left - self.scroll.x,
            y: self.row_top(row),
        }
    }

    /// The area right of the gutter, which text is clipped to.
    pub fn text_bounds(&self) -> Rect {
        Rect {
            x: self.text_left,
            y: 0.,
            width: self.width - self.text_left,
            height: self.height,
        }
    }

    /// The buffer offset under `position`. Rows outside this frame (dragging
    /// past an edge) clamp to its first or last row, and positions past the
    /// end of a soft-wrapped row stay on it.
    pub fn offset_at(&self, text: &Text, position: Vec2) -> usize {
        let row = ((position.y + self.scroll.y) / self.line_height)
            .floor()
            .max(0.) as u32;
        if row >= self.row_count {
            return text.len();
        }
        let Some(row) = self.row(row).or_else(|| {
            if row < self.first_row {
                self.rows.first()
            } else {
                self.rows.last()
            }
        }) else {
            return 0;
        };
        let display = row
            .line
            .index_for_x(position.x - self.text_left + self.scroll.x);
        let offset = row.start + row.expanded.to_buffer(display, Bias::Left);
        if !row.last && offset >= row.end {
            return text.clip_offset(row.end - 1, Bias::Left).max(row.start);
        }
        offset.min(row.end)
    }

    /// A caret-sized rectangle at `offset`, if its row is in this frame.
    pub fn caret_bounds(&self, offset: usize) -> Option<Rect> {
        let row = self.row_containing(offset)?;
        Some(Rect {
            x: self.text_left + row.x_for_offset(offset) - self.scroll.x,
            y: self.row_top(row.row),
            width: CURSOR_WIDTH,
            height: self.line_height,
        })
    }
}

const CURSOR_WIDTH: f32 = 2.;

/// The gutter's width, which fits the line numbers of `buffer_rows` rows.
pub fn gutter_width(buffer_rows: u32, em: f32) -> f32 {
    let digits = buffer_rows.to_string().len().max(3) as f32;
    em * (digits + 4.)
}

/// The width to soft wrap at in an editor `width` wide: what's right of the
/// gutter, less a margin, in whole ems. Re-wrapping a large file takes a
/// while, so resizing only does it when another em fits.
pub fn wrap_width(width: f32, buffer_rows: u32, em: f32) -> f32 {
    let available = width - gutter_width(buffer_rows, em) - em * 2.;
    (available / em).floor().max(1.) * em
}

/// Lays out the rows visible in `input`, shaping them with `measure`.
/// `highlights` returns the styled spans within a buffer range, sorted; it's
/// called once per run of consecutive visible rows.
pub fn layout_frame<M: TextMeasure>(
    input: &FrameInput<M::Style>,
    measure: &mut M,
    mut highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<M::Style>>,
) -> Frame<M::Line> {
    let FrameInput {
        display,
        tab_size,
        selections,
        primary_head,
        mut scroll,
        autoscroll,
        width,
        height,
        line_height,
        em,
        ref styles,
    } = *input;
    let text = display.text();

    let row_count = display.row_count();
    let text_left = gutter_width(text.row_count(), em);
    let text_width = width - text_left;

    // Keep the primary cursor a few rows from the top and bottom edges.
    if autoscroll {
        let margin = (line_height * 3.).min(height / 3.);
        let head_top = line_height * display.row_for_offset(primary_head) as f32;
        if head_top < scroll.y + margin {
            scroll.y = head_top - margin;
        }
        if head_top + line_height > scroll.y + height - margin {
            scroll.y = head_top + line_height - height + margin;
        }
    }
    // Allow scrolling until the last row reaches the top.
    let max_scroll_y = line_height * row_count.saturating_sub(1) as f32;
    scroll.y = scroll.y.clamp(0., max_scroll_y);

    let first_row = (scroll.y / line_height).floor() as u32;
    let end_row = (((scroll.y + height) / line_height).ceil() as u32).min(row_count);
    let row_top = |row: u32| line_height * row as f32 - scroll.y;

    let display_rows: Vec<_> = (first_row..end_row).map(|row| display.row(row)).collect();
    let shown: Vec<Range<usize>> = display_rows
        .iter()
        .map(|row| {
            let len = row.range.len().min(MAX_SHAPED_ROW_BYTES);
            row.range.start..text.clip_offset(row.range.start + len, Bias::Left)
        })
        .collect();
    let spans = visible_spans(&shown, &mut highlights);

    let mut rows = Vec::with_capacity(shown.len());
    let mut folded = Vec::new();
    for ((row, display_row), range) in (first_row..).zip(display_rows).zip(shown) {
        let expanded = ExpandedRow::new(&text.slice_to_string(range.clone()), tab_size);
        let first_span = spans.partition_point(|span| span.range.end <= range.start);
        let runs = row_runs(&expanded, range.start, &spans[first_span..], &styles.text);
        let line = measure.shape(expanded.text(), &runs);
        if display_row.folded {
            folded.push(rows.len());
        }
        rows.push(RowLayout {
            row,
            buffer_row: display_row.buffer_row,
            start: display_row.range.start,
            end: display_row.range.end,
            last: display_row.last,
            expanded,
            line,
        });
    }
    let containing = |offset: usize| rows.iter().find(|row| row.contains(offset));

    // Horizontally, keep the cursor inside the text area too. Wrapped rows
    // fit, so they don't scroll.
    if display.wrap_width().is_some() {
        scroll.x = 0.;
    } else {
        if autoscroll && let Some(row) = containing(primary_head) {
            let x = row.x_for_offset(primary_head);
            let margin = em * 4.;
            if x < scroll.x + margin {
                scroll.x = x - margin;
            }
            if x > scroll.x + text_width - margin {
                scroll.x = x - text_width + margin;
            }
        }
        let widest = rows.iter().map(|row| row.line.width()).fold(0., f32::max);
        let max_scroll_x = (widest - text_width + em * 4.).max(0.);
        scroll.x = scroll.x.clamp(0., max_scroll_x);
    }
    let x_at =
        |row: &RowLayout<M::Line>, offset: usize| text_left + row.x_for_offset(offset) - scroll.x;

    let mut selection_rects = Vec::new();
    let mut cursors = Vec::new();
    // Display rows with a caret, and their buffer rows.
    let mut active_rows = Vec::new();
    let mut active_buffer_rows = Vec::new();
    for selection in selections {
        let head = selection.head();
        let head_row = containing(head);
        if let Some(row) = head_row {
            cursors.push(Rect {
                x: x_at(row, head),
                y: row_top(row.row),
                width: CURSOR_WIDTH,
                height: line_height,
            });
        }
        if selection.range.is_empty() {
            if let Some(row) = head_row {
                active_rows.push(row.row);
                active_buffer_rows.push(row.buffer_row);
            }
            continue;
        }
        for row in &rows {
            let from = selection.range.start.max(row.start);
            let to = selection.range.end.min(row.end);
            // Show that the line break (and any rows folded after it) is
            // selected too.
            let line_break = row.last && selection.range.end > row.end;
            if from > to || from == to && !line_break {
                continue;
            }
            let mut right = x_at(row, to);
            if line_break {
                right += em * 0.6;
            }
            selection_rects.push(Rect::from_corners(
                x_at(row, from),
                row_top(row.row),
                right,
                row_top(row.row) + line_height,
            ));
        }
    }
    active_rows.sort_unstable();
    active_rows.dedup();
    active_buffer_rows.sort_unstable();
    let active_lines = active_rows
        .iter()
        .map(|&row| Rect {
            x: 0.,
            y: row_top(row),
            width,
            height: line_height,
        })
        .collect();

    let mut shape = |text: &str, style: &M::Style| {
        let run = StyleRun {
            len: text.len(),
            style: style.clone(),
        };
        measure.shape(text, &[run])
    };
    let line_numbers = rows
        .iter()
        .filter(|row| row.start == text.row_start(row.buffer_row))
        .map(|row| {
            let style = if active_buffer_rows.binary_search(&row.buffer_row).is_ok() {
                &styles.active_line_number
            } else {
                &styles.line_number
            };
            let line = shape(&(row.buffer_row + 1).to_string(), style);
            let origin = Vec2 {
                x: text_left - em * 2. - line.width(),
                y: row_top(row.row),
            };
            (origin, line)
        })
        .collect();
    let fold_markers = folded
        .into_iter()
        .map(|ix| {
            let row = &rows[ix];
            let origin = Vec2 {
                x: text_left + row.line.width() + em - scroll.x,
                y: row_top(row.row),
            };
            (origin, shape(FOLD_MARKER, &styles.line_number))
        })
        .collect();

    Frame {
        width,
        height,
        line_height,
        scroll,
        text_left,
        row_count,
        first_row,
        rows,
        active_lines,
        selections: selection_rects,
        cursors,
        line_numbers,
        fold_markers,
    }
}

/// Spans for the visible rows. Consecutive rows (across a line break or a
/// soft wrap) are queried together; a row cut short ends the query, so the
/// hidden rest of a long line isn't highlighted.
fn visible_spans<S>(
    rows: &[Range<usize>],
    highlights: &mut impl FnMut(Range<usize>) -> Vec<StyledSpan<S>>,
) -> Vec<StyledSpan<S>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for row in rows {
        match ranges.last_mut() {
            Some(last) if row.start == last.end || row.start == last.end + 1 => last.end = row.end,
            _ => ranges.push(row.clone()),
        }
    }
    ranges.into_iter().flat_map(highlights).collect()
}

/// Style runs covering a row's displayed text: spans in their own style and
/// everything else in `text`. `spans` are sorted and start with the first
/// one ending after `row_start`.
pub fn row_runs<S: Clone>(
    expanded: &ExpandedRow,
    row_start: usize,
    spans: &[StyledSpan<S>],
    text: &S,
) -> Vec<StyleRun<S>> {
    let shown = expanded.text().len();
    let display = |offset: usize| {
        expanded
            .to_display(offset.saturating_sub(row_start))
            .min(shown)
    };
    let run = |len, style: &S| StyleRun {
        len,
        style: style.clone(),
    };
    let mut runs = Vec::new();
    let mut at = 0;
    for span in spans {
        if at == shown {
            break;
        }
        let (start, end) = (display(span.range.start).max(at), display(span.range.end));
        if end <= start {
            continue;
        }
        if start > at {
            runs.push(run(start - at, text));
        }
        runs.push(run(end - start, &span.style));
        at = end;
    }
    if at < shown {
        runs.push(run(shown - at, text));
    }
    debug_assert_eq!(
        runs.iter().map(|run| run.len).sum::<usize>(),
        shown,
        "runs must cover {:?}",
        expanded.text()
    );
    runs
}

#[cfg(test)]
mod tests;
