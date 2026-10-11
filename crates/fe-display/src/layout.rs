//! Lays out one frame of an editor: scrolling, the visible display rows and
//! their style runs, the blocks between them, selection and cursor
//! rectangles, line numbers, fold toggles and fold markers.
//!
//! This is plain arithmetic over a [`TextMeasure`], so it runs without a
//! window. The UI implements the trait with its text shaper, then paints what
//! [`layout_frame`] returns; tests use a fake monospace measurer.
//!
//! Positions are in pixels from the editor's top-left corner. Rows are placed
//! by screen row: display rows and the block rows between them.

use std::ops::Range;

use fe_buffer::ResolvedSelection;
use fe_text::{Bias, TextSource};

use crate::{
    BlockMap, CharWidth, DisplayMap, DisplayRow, ExpandedRow, LongRows, RowIndex, RowSlice,
    ScreenRow,
};

/// Longest row that is shaped whole. Of a longer row (minified files, data
/// dumps), only a slice around the view is shaped, rather than stalling a
/// frame, and never more than this many bytes of it at once; with soft wrap
/// on, such a row is still to be wrapped and only its first this many bytes
/// are shaped. Bytes are buffer bytes: tabs expand when shaped.
pub const MAX_SHAPED_ROW_BYTES: usize = 8 * 1024;

/// How far past each side of the view a long row's slice reaches, in ems.
const SLICE_MARGIN_EMS: f32 = 32.;

/// Shown after a row whose following rows are folded.
pub const FOLD_MARKER: &str = "⋯";

/// The width of the fold column, the strip between the line numbers and the
/// text where chevrons go, in ems.
pub const FOLD_COLUMN_EMS: f32 = 3.75;
/// The side of a chevron's box, in ems, padded by the rest of the column on
/// either side (0.75 em each).
pub const FOLD_CHEVRON_EMS: f32 = 2.25;
/// The most of a row's height a chevron's box takes, so it fits short rows.
pub const FOLD_CHEVRON_LINE_FRACTION: f32 = 0.875;

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
    /// Whether `point` is inside, counting the top and left edges.
    pub fn contains(&self, point: Vec2) -> bool {
        (self.x..self.x + self.width).contains(&point.x)
            && (self.y..self.y + self.height).contains(&point.y)
    }

    /// The part inside `other`, if any.
    pub fn intersect(&self, other: Rect) -> Option<Rect> {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        (x0 < x1 && y0 < y1).then(|| Self::from_corners(x0, y0, x1, y1))
    }

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
    /// Display rows and block rows in the document.
    pub screen_rows: u32,
    /// The blocks the frame was laid out with.
    pub block_map: BlockMap,
    /// The first display row in the frame.
    pub first_row: u32,
    pub rows: Vec<RowLayout<L>>,
    /// The blocks at least partly in view, in order.
    pub blocks: Vec<BlockLayout>,
    /// The screen rows of each of `blocks`, and the buffer offset a click
    /// on it goes to.
    block_offsets: Vec<(Range<u32>, usize)>,
    /// Full-width bands behind rows that have a caret.
    pub active_lines: Vec<Rect>,
    pub selections: Vec<Rect>,
    pub cursors: Vec<Rect>,
    pub line_numbers: Vec<(Vec2, L)>,
    /// The strip between the line numbers and the text, the frame's height,
    /// where chevrons go.
    pub fold_column: Rect,
    /// The side of a chevron's square box.
    pub fold_chevron: f32,
    /// Chevrons in the gutter of the rows that can fold or are folded.
    pub fold_toggles: Vec<FoldToggle>,
    /// [`FOLD_MARKER`]s after rows with folded rows below.
    pub fold_markers: Vec<FoldMarker<L>>,
    /// Where the inlay was laid out, if its row is in the frame.
    pub inlay: Option<Rect>,
}

/// Text drawn inside a row at a buffer offset, with the rest of the row and
/// every position on it shifted past it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inlay<S> {
    pub offset: usize,
    /// One line.
    pub text: String,
    pub style: S,
}

/// Where a chevron that folds or unfolds a buffer row goes: in the gutter
/// between its line number and the text.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FoldToggle {
    pub rect: Rect,
    pub buffer_row: u32,
    /// The row heads a fold; otherwise it heads a fold candidate.
    pub folded: bool,
}

/// A [`FOLD_MARKER`] after a folded row.
#[derive(Clone, Debug)]
pub struct FoldMarker<L> {
    /// Covers the marker, where it's painted.
    pub rect: Rect,
    pub buffer_row: u32,
    pub line: L,
}

/// Where a block's rows go: across the text area, between display rows.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BlockLayout {
    pub id: u64,
    pub rows: u32,
    pub rect: Rect,
}

/// A display row: all of a buffer row or, soft wrapped, part of it.
#[derive(Clone, Debug)]
pub struct RowLayout<L> {
    /// The display row.
    pub row: u32,
    pub buffer_row: u32,
    /// The buffer offsets the row covers.
    pub start: usize,
    pub end: usize,
    /// Ends its buffer row, rather than at a soft wrap.
    pub last: bool,
    /// Of a row longer than [`MAX_SHAPED_ROW_BYTES`] with soft wrap off, the
    /// part around the view that's shaped. Positions elsewhere in the row
    /// come from its [`RowIndex`] in the frame's [`LongRows`].
    pub slice: Option<RowSlice>,
    /// The shaped text, from [`RowLayout::shown`].
    pub expanded: ExpandedRow,
    pub line: L,
    /// The width of the inlay drawn in `line`, which the row's index doesn't
    /// know: text of a long row past the shaped slice sits that far right of
    /// where the index puts it.
    pub inlay_width: f64,
}

impl<L> RowLayout<L> {
    /// The buffer offsets shaped into `line`: the row's slice, or the start
    /// of the row up to [`MAX_SHAPED_ROW_BYTES`].
    pub fn shown(&self) -> Range<usize> {
        match &self.slice {
            Some(slice) => slice.range.clone(),
            None => {
                let shown = self.expanded.text().len();
                self.start..self.start + self.expanded.to_buffer(shown, Bias::Left)
            }
        }
    }

    /// The x of `line`'s start, from column 0.
    pub fn start_x(&self) -> f64 {
        self.slice.as_ref().map_or(0., |slice| slice.start_x)
    }

    /// Whether the caret at `offset` is on this row. A soft wrap point
    /// starts the row after it.
    pub fn contains(&self, offset: usize) -> bool {
        contains(self.start..self.end, self.last, offset)
    }

    /// A long row's index, if `long_rows` still holds it for this row.
    fn index<'a>(&self, long_rows: &'a mut LongRows) -> Option<&'a mut RowIndex> {
        self.slice.as_ref()?;
        long_rows
            .get(self.buffer_row)
            .filter(|index| index.range() == (self.start..self.end))
    }
}

impl<L: MeasuredLine> RowLayout<L> {
    /// X offset (from column 0) of a buffer offset in this row. Past the
    /// shaped text, a long row's index places it, if `long_rows` holds it.
    pub fn x_for_offset(
        &self,
        offset: usize,
        long_rows: &mut LongRows,
        widths: &mut dyn CharWidth,
    ) -> f64 {
        let shown = self.shown();
        if !(shown.start..=shown.end).contains(&offset)
            && let Some(index) = self.index(long_rows)
        {
            let shift = if offset > shown.end {
                self.inlay_width
            } else {
                0.
            };
            return index.x_for_offset(offset, widths) + shift;
        }
        let display = self
            .expanded
            .to_display(offset.clamp(shown.start, shown.end) - shown.start);
        self.start_x() + f64::from(self.line.x_for_index(display))
    }

    /// The buffer offset closest to `x` (from column 0) in this row.
    fn offset_for_x(&self, x: f64, long_rows: &mut LongRows, widths: &mut dyn CharWidth) -> usize {
        let line_x = x - self.start_x();
        // Past the shaped text, the index finds the offset, unless the
        // shaped text runs to the row's end that way: then x is past the
        // row, even where the index measures the row wider than it's drawn.
        let shown = self.shown();
        if (line_x < 0. && shown.start > self.start
            || line_x > f64::from(self.line.width()) && shown.end < self.end)
            && let Some(index) = self.index(long_rows)
        {
            // Right of the slice, the index's x is short of the drawn x by
            // the inlay.
            let x = if line_x > 0. { x - self.inlay_width } else { x };
            let before = index.offset_for_x(x, widths);
            let after = index.next_boundary(before, widths).min(self.end);
            let past = x - index.x_for_offset(before, widths);
            return if after > before && index.x_for_offset(after, widths) - x < past {
                after
            } else {
                before
            };
        }
        let display = self.line.index_for_x(line_x as f32);
        shown.start + self.expanded.to_buffer(display, Bias::Left)
    }
}

/// A row laid out, or a long row to slice once the horizontal scroll is
/// known, with its display row.
type Pending<L> = Result<RowLayout<L>, (u32, DisplayRow)>;

fn contains(row: Range<usize>, last: bool, offset: usize) -> bool {
    row.start <= offset && (offset < row.end || offset == row.end && last)
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

    /// The top of a display row, below the blocks above it.
    pub fn row_top(&self, row: u32) -> f32 {
        self.line_height * self.block_map.screen_row(row) as f32 - self.scroll.y
    }

    /// What's shown at `y`: a display row or a block row, in this frame or
    /// past its edges.
    pub fn screen_row_at(&self, y: f32) -> ScreenRow {
        self.block_map.at_screen_row(self.screen_row_index(y))
    }

    fn screen_row_index(&self, y: f32) -> u32 {
        ((y + self.scroll.y) / self.line_height).floor().max(0.) as u32
    }

    /// The x in the frame of `x` from column 0. Far along a long row, x is
    /// too big for an `f32` to place exactly, so this subtracts the scroll
    /// position first.
    pub fn view_x(&self, x: f64) -> f32 {
        self.text_left + (x - f64::from(self.scroll.x)) as f32
    }

    /// Where a row's line is painted.
    pub fn line_origin(&self, row: &RowLayout<L>) -> Vec2 {
        Vec2 {
            x: self.view_x(row.start_x()),
            y: self.row_top(row.row),
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

    /// The square a toggle's chevron is drawn in, centred on its rect: the
    /// fold column's centre across and the row's centre down.
    pub fn chevron_box(&self, toggle: &FoldToggle) -> Rect {
        let side = self.fold_chevron;
        let rect = toggle.rect;
        Rect {
            x: rect.x + (rect.width - side) / 2.,
            y: rect.y + (rect.height - side) / 2.,
            width: side,
            height: side,
        }
    }

    /// Where clicks fold or unfold rows: the fold toggles, then the fold
    /// markers as folded toggles, each clipped to what's shown of it.
    pub fn fold_click_areas(&self) -> impl Iterator<Item = FoldToggle> + '_ {
        let bounds = Rect {
            x: 0.,
            y: 0.,
            width: self.width,
            height: self.height,
        };
        let markers = self.fold_markers.iter().map(|marker| FoldToggle {
            rect: marker.rect,
            buffer_row: marker.buffer_row,
            folded: true,
        });
        let text = self.text_bounds();
        (self
            .fold_toggles
            .iter()
            .map(move |toggle| (*toggle, bounds)))
        .chain(markers.map(move |marker| (marker, text)))
        .filter_map(|(area, shown)| {
            let rect = area.rect.intersect(shown)?;
            Some(FoldToggle { rect, ..area })
        })
    }

    /// The buffer offset under `position`. Rows outside this frame (dragging
    /// past an edge) clamp to its first or last row, and positions past the
    /// end of a soft-wrapped row stay on it. A block in this frame gives the
    /// end of the display row above it, or the start of the first row;
    /// outside it, a block row clamps like the display row below it. A frame
    /// with only block rows clamps to the nearest block's anchor.
    /// `long_rows` and `widths` are what the frame was laid out with.
    pub fn offset_at(
        &self,
        text: &dyn TextSource,
        position: Vec2,
        long_rows: &mut LongRows,
        widths: &mut dyn CharWidth,
    ) -> usize {
        let screen_row = self.screen_row_index(position.y);
        if let Some((_, offset)) = self
            .block_offsets
            .iter()
            .find(|(rows, _)| rows.contains(&screen_row))
        {
            return *offset;
        }
        let row = self.block_map.display_row_from(screen_row);
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
            // A viewport filled by blocks has no text rows to clamp to; the
            // nearest block's anchor is the closest boundary.
            let block = if row < self.first_row {
                self.block_offsets.first()
            } else {
                self.block_offsets.last()
            };
            return block.map_or(0, |(_, offset)| *offset);
        };
        let x = f64::from(position.x - self.text_left) + f64::from(self.scroll.x);
        let offset = row.offset_for_x(x, long_rows, widths);
        if !row.last && offset >= row.end {
            return text.clip_offset(row.end - 1, Bias::Left).max(row.start);
        }
        offset.min(row.end)
    }

    /// A caret-sized rectangle at `offset`, if its row is in this frame.
    pub fn caret_bounds(
        &self,
        offset: usize,
        long_rows: &mut LongRows,
        widths: &mut dyn CharWidth,
    ) -> Option<Rect> {
        let row = self.row_containing(offset)?;
        Some(Rect {
            x: self.view_x(row.x_for_offset(offset, long_rows, widths)),
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
    em * (digits + 2. + FOLD_COLUMN_EMS)
}

/// The width to soft wrap at in an editor `width` wide: what's right of the
/// gutter, less a margin, in whole ems. Re-wrapping a large file takes a
/// while, so resizing only does it when another em fits.
pub fn wrap_width(width: f32, buffer_rows: u32, em: f32) -> f32 {
    let available = width - gutter_width(buffer_rows, em) - em * 2.;
    (available / em).floor().max(1.) * em
}

/// Lays out the rows visible in `input`, with no blocks: see
/// [`layout_frame_with_blocks`].
pub fn layout_frame<M: TextMeasure>(
    input: &FrameInput<M::Style>,
    measure: &mut M,
    long_rows: &mut LongRows,
    widths: &mut dyn CharWidth,
    highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<M::Style>>,
    fold_candidates: impl FnOnce(Range<u32>) -> Vec<u32>,
) -> Frame<M::Line> {
    layout_frame_with_blocks(
        input,
        &BlockMap::default(),
        measure,
        long_rows,
        widths,
        highlights,
        fold_candidates,
    )
}

/// Lays out the rows visible in `input`, shaping them with `measure`, and
/// places `blocks` between them.
/// `highlights` returns the styled spans within a buffer range, sorted; it's
/// called once per run of consecutive visible rows, and once per slice of a
/// long row, again each time the slice is widened. `fold_candidates` returns
/// the buffer rows within a range that head a fold candidate; it's called
/// once, with the visible buffer rows.
/// `long_rows` holds the indexes of long rows, measured with `widths`, from
/// frame to frame.
pub fn layout_frame_with_blocks<M: TextMeasure>(
    input: &FrameInput<M::Style>,
    blocks: &BlockMap,
    measure: &mut M,
    long_rows: &mut LongRows,
    widths: &mut dyn CharWidth,
    highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<M::Style>>,
    fold_candidates: impl FnOnce(Range<u32>) -> Vec<u32>,
) -> Frame<M::Line> {
    layout_frame_with_inlay(
        input,
        blocks,
        None,
        measure,
        long_rows,
        widths,
        highlights,
        fold_candidates,
    )
}

/// Lays out like [`layout_frame_with_blocks`], with `inlay` drawn inside the
/// display row that holds its offset, if the offset is in what's shaped of
/// the row. The row isn't wrapped for it, so it may run past the wrap width.
#[allow(clippy::too_many_arguments)]
pub fn layout_frame_with_inlay<M: TextMeasure>(
    input: &FrameInput<M::Style>,
    blocks: &BlockMap,
    inlay: Option<&Inlay<M::Style>>,
    measure: &mut M,
    long_rows: &mut LongRows,
    widths: &mut dyn CharWidth,
    mut highlights: impl FnMut(Range<usize>) -> Vec<StyledSpan<M::Style>>,
    fold_candidates: impl FnOnce(Range<u32>) -> Vec<u32>,
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
    let source = display.source_key();
    long_rows.start_frame();

    let row_count = display.row_count();
    let screen_rows = row_count + blocks.block_rows();
    let text_left = gutter_width(text.row_count(), em);
    let fold_column_width = em * FOLD_COLUMN_EMS;
    let text_width = width - text_left;

    // Keep the primary cursor a few rows from the top and bottom edges.
    if autoscroll {
        let margin = (line_height * 3.).min(height / 3.);
        let head_row = blocks.screen_row(display.row_for_offset(primary_head));
        let head_top = line_height * head_row as f32;
        if head_top < scroll.y + margin {
            scroll.y = head_top - margin;
        }
        if head_top + line_height > scroll.y + height - margin {
            scroll.y = head_top + line_height - height + margin;
        }
    }
    // Allow scrolling until the last row, or block row, reaches the top.
    let max_scroll_y = line_height * screen_rows.saturating_sub(1) as f32;
    scroll.y = scroll.y.clamp(0., max_scroll_y);

    let first_screen_row = (scroll.y / line_height).floor() as u32;
    let end_screen_row = ((scroll.y + height) / line_height).ceil() as u32;
    let first_row = blocks.display_row_from(first_screen_row).min(row_count);
    let end_row = blocks.display_row_from(end_screen_row).min(row_count);
    let row_top = |row: u32| line_height * blocks.screen_row(row) as f32 - scroll.y;

    // Long rows are sliced once the horizontal scroll is known; the others
    // are shaped first, since it depends on them. With soft wrap on, a row
    // too long to shape is still to be wrapped, and is cut off instead.
    let wrapped = display.wrap_width().is_some();
    let is_long = |row: &DisplayRow| !wrapped && row.range.len() > MAX_SHAPED_ROW_BYTES;
    let display_rows: Vec<_> = (first_row..end_row).map(|row| display.row(row)).collect();
    let folded: Vec<usize> = (0..display_rows.len())
        .filter(|&ix| display_rows[ix].folded)
        .collect();
    let cut = |row: &DisplayRow| {
        let len = row.range.len().min(MAX_SHAPED_ROW_BYTES);
        row.range.start..text.clip_offset(row.range.start + len, Bias::Left)
    };
    let shown: Vec<_> = display_rows
        .iter()
        .filter(|row| !is_long(row))
        .map(|row| (row.buffer_row, cut(row)))
        .collect();
    let spans = visible_spans(text, &shown, &mut highlights);

    let shape_row = |measure: &mut M,
                     row: u32,
                     display_row: DisplayRow,
                     slice: Option<RowSlice>,
                     spans: &[StyledSpan<M::Style>]| {
        let (shown, start_column) = match &slice {
            Some(slice) => (slice.range.clone(), slice.start_column),
            None => (cut(&display_row), 0),
        };
        let expanded = ExpandedRow::with_start_column(
            &text.slice_to_string(shown.clone()),
            start_column,
            tab_size,
        );
        let first_span = spans.partition_point(|span| span.range.end <= shown.start);
        let mut expanded = expanded;
        let mut runs = row_runs(&expanded, shown.start, &spans[first_span..], &styles.text);
        if let Some(inlay) = inlay
            && contains(display_row.range.clone(), display_row.last, inlay.offset)
            && (shown.start..=shown.end).contains(&inlay.offset)
        {
            expanded.insert_inlay(inlay.offset - shown.start, &inlay.text);
            if let Some(range) = expanded.inlay_range() {
                insert_run(&mut runs, range.start, inlay.text.len(), &inlay.style);
            }
        }
        let line = measure.shape(expanded.text(), &runs);
        let inlay_width = expanded.inlay_range().map_or(0., |range| {
            f64::from(line.x_for_index(range.end) - line.x_for_index(range.start))
        });
        RowLayout {
            row,
            buffer_row: display_row.buffer_row,
            start: display_row.range.start,
            end: display_row.range.end,
            last: display_row.last,
            slice,
            expanded,
            line,
            inlay_width,
        }
    };
    let laid_out: Vec<Pending<M::Line>> = (first_row..)
        .zip(display_rows)
        .map(|(row, display_row)| {
            if is_long(&display_row) {
                Err((row, display_row))
            } else {
                Ok(shape_row(measure, row, display_row, None, &spans))
            }
        })
        .collect();

    // Horizontally, keep the cursor inside the text area too. Wrapped rows
    // fit, so they don't scroll. Long rows are as wide as their index
    // measures them, though only a slice is shaped.
    if wrapped {
        scroll.x = 0.;
    } else {
        let text_width = f64::from(text_width);
        let margin = f64::from(em) * 4.;
        let slice_margin = f64::from(em * SLICE_MARGIN_EMS);
        // What the inlay adds to a long row's width: its own width, if its
        // row is long and it's in the slice shaped for a view at `scroll_x`.
        let inlay_text_width = inlay.map(|inlay| {
            let runs = [StyleRun {
                len: inlay.text.len(),
                style: inlay.style.clone(),
            }];
            f64::from(measure.shape(&inlay.text, &runs).width())
        });
        let inlay_in_slice = |long_rows: &mut LongRows,
                              widths: &mut dyn CharWidth,
                              row: &DisplayRow,
                              scroll_x: f64| {
            let (Some(inlay), Some(width)) = (inlay, inlay_text_width) else {
                return 0.;
            };
            if !contains(row.range.clone(), row.last, inlay.offset) {
                return 0.;
            }
            let view = scroll_x..scroll_x + text_width;
            let slice = long_rows
                .index(source, text, row.buffer_row, tab_size, widths)
                .slice(view, slice_margin, widths);
            if (slice.range.start..=slice.range.end).contains(&inlay.offset) {
                width
            } else {
                0.
            }
        };
        let long_head = laid_out.iter().find_map(|row| match row {
            Err((_, row)) if contains(row.range.clone(), row.last, primary_head) => Some(row),
            _ => None,
        });
        // The caret's x in a long row at `scroll_x`: past the inlay, the row
        // is wider by it.
        let long_head_x = |long_rows: &mut LongRows,
                           widths: &mut dyn CharWidth,
                           row: &DisplayRow,
                           scroll_x: f64| {
            let x = long_rows
                .index(source, text, row.buffer_row, tab_size, widths)
                .x_for_offset(primary_head, widths);
            let past = inlay.is_some_and(|inlay| primary_head > inlay.offset);
            if past {
                x + inlay_in_slice(long_rows, widths, row, scroll_x)
            } else {
                x
            }
        };
        let follow = |x: f64, mut scroll_x: f64| {
            if x < scroll_x + margin {
                scroll_x = x - margin;
            }
            if x > scroll_x + text_width - margin {
                scroll_x = x - text_width + margin;
            }
            scroll_x
        };
        let mut scroll_x = f64::from(scroll.x);
        if autoscroll {
            if let Some(row) = long_head {
                // Scroll to the caret as the index places it; if the inlay
                // is in the slice there, scroll to the caret past it, and
                // keep that if the inlay's still in the slice.
                let plain_x = long_rows
                    .index(source, text, row.buffer_row, tab_size, widths)
                    .x_for_offset(primary_head, widths);
                let plain = follow(plain_x, scroll_x);
                let shifted = follow(long_head_x(long_rows, widths, row, plain), scroll_x);
                scroll_x =
                    if shifted != plain && long_head_x(long_rows, widths, row, shifted) > plain_x {
                        shifted
                    } else {
                        plain
                    };
            } else if let Some(x) = laid_out.iter().find_map(|row| match row {
                Ok(row) if row.contains(primary_head) => {
                    Some(row.x_for_offset(primary_head, long_rows, widths))
                }
                _ => None,
            }) {
                scroll_x = follow(x, scroll_x);
            }
        }
        let mut widest = 0_f64;
        for row in &laid_out {
            let width = match row {
                Ok(row) => f64::from(row.line.width()),
                Err((_, row)) => {
                    let width = long_rows
                        .index(source, text, row.buffer_row, tab_size, widths)
                        .width(widths);
                    width + inlay_in_slice(long_rows, widths, row, scroll_x)
                }
            };
            widest = widest.max(width);
        }
        let max_scroll_x = (widest - text_width + margin).max(0.);
        scroll.x = scroll_x.clamp(0., max_scroll_x) as f32;
    }

    let scroll_x = f64::from(scroll.x);
    let view = scroll_x..scroll_x + f64::from(text_width);
    let slice_margin = f64::from(em * SLICE_MARGIN_EMS);
    let rows: Vec<_> = laid_out
        .into_iter()
        .map(|row| match row {
            Ok(row) => row,
            Err((row, display_row)) => {
                let index = long_rows.index(source, text, display_row.buffer_row, tab_size, widths);
                let mut slice = index.slice(view.clone(), slice_margin, widths);
                let mut shape_slice = |slice: &RowSlice| {
                    let spans = highlights(slice.range.clone());
                    shape_row(
                        measure,
                        row,
                        display_row.clone(),
                        Some(slice.clone()),
                        &spans,
                    )
                };
                let mut layout = shape_slice(&slice);
                // Where the shaper draws the slice narrower than the index
                // measures it (contextual forms the index doesn't know), its
                // text ends short of the view. Widen it by twice the
                // shortfall, in index units per shaped unit over the slice,
                // which covers the view and margin if the text added is
                // drawn at least half as wide, relative to the index, as the
                // slice's. Nothing shaped so far bounds how narrow the text
                // after it is drawn, so if the text still ends short of the
                // view's right edge, the third and last shape takes as much
                // as a slice may hold: up to the row's end or
                // `MAX_SHAPED_ROW_BYTES`, whichever is nearer. Shaping stays
                // bounded whatever the shaper does; text drawn so narrow that
                // even that ends short of the view leaves the rest of the
                // view blank for the frame.
                let shaped = f64::from(layout.line.width());
                let short = view.end + slice_margin - (slice.start_x + shaped);
                let limit = if short > 0. {
                    index.slice_limit(slice.range.start, widths)
                } else {
                    slice.range.end
                };
                if slice.range.end < limit {
                    slice = if shaped > 0. {
                        let indexed = index.x_for_offset(slice.range.end, widths) - slice.start_x;
                        index.widen(&slice, 2. * short * indexed / shaped, widths)
                    } else {
                        // Text shaped to nothing gives no ratio.
                        RowSlice {
                            range: slice.range.start..limit,
                            ..slice
                        }
                    };
                    layout = shape_slice(&slice);
                    if slice.start_x + f64::from(layout.line.width()) < view.end
                        && slice.range.end < limit
                    {
                        slice.range.end = limit;
                        layout = shape_slice(&slice);
                    }
                }
                layout
            }
        })
        .collect();
    let containing = |offset: usize| rows.iter().find(|row| row.contains(offset));
    let mut x_at = |row: &RowLayout<M::Line>, offset: usize| {
        text_left + (row.x_for_offset(offset, long_rows, widths) - scroll_x) as f32
    };

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
            // Far along a long row, a selection reaches far off screen,
            // further than an `f32` places its visible edge exactly.
            let clamp = |x: f32| x.clamp(text_left - em, width + em);
            selection_rects.push(Rect::from_corners(
                clamp(x_at(row, from)),
                row_top(row.row),
                clamp(right),
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
    // Rows that start their buffer row.
    let first_rows = || {
        rows.iter()
            .filter(|row| row.start == text.row_start(row.buffer_row))
    };
    let line_numbers = first_rows()
        .map(|row| {
            let style = if active_buffer_rows.binary_search(&row.buffer_row).is_ok() {
                &styles.active_line_number
            } else {
                &styles.line_number
            };
            let line = shape(&(row.buffer_row + 1).to_string(), style);
            let origin = Vec2 {
                x: text_left - fold_column_width - line.width(),
                y: row_top(row.row),
            };
            (origin, line)
        })
        .collect();
    let fold_markers = folded
        .into_iter()
        .map(|ix| {
            let row = &rows[ix];
            let line = shape(FOLD_MARKER, &styles.line_number);
            FoldMarker {
                rect: Rect {
                    x: x_at(row, row.end) + em,
                    y: row_top(row.row),
                    width: line.width(),
                    height: line_height,
                },
                buffer_row: row.buffer_row,
                line,
            }
        })
        .collect();

    let inlay_rect = rows.iter().find_map(|row| {
        let range = row.expanded.inlay_range()?;
        let x = |index: usize| row.start_x() + f64::from(row.line.x_for_index(index));
        Some(Rect {
            x: text_left + (x(range.start) - scroll_x) as f32,
            y: row_top(row.row),
            width: (x(range.end) - x(range.start)) as f32,
            height: line_height,
        })
    });

    let visible_buffer_rows = match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => first.buffer_row..last.buffer_row + 1,
        _ => 0..0,
    };
    let mut candidates = fold_candidates(visible_buffer_rows);
    candidates.sort_unstable();
    // A header wrapped onto several rows is folded even when the row that
    // shows its marker is out of view.
    let heads_fold = |buffer_row: u32| {
        buffer_row + 1 < text.row_count() && display.hidden_rows(buffer_row + 1).is_some()
    };
    let fold_toggles = first_rows()
        .filter_map(|row| {
            let folded = heads_fold(row.buffer_row);
            (folded || candidates.binary_search(&row.buffer_row).is_ok()).then(|| FoldToggle {
                rect: Rect {
                    x: text_left - fold_column_width,
                    y: row_top(row.row),
                    width: fold_column_width,
                    height: line_height,
                },
                buffer_row: row.buffer_row,
                folded,
            })
        })
        .collect();

    // A click on a block goes to the end of the row above it.
    let row_end = |row: u32| {
        let row = display.row(row);
        if row.last {
            row.range.end
        } else {
            text.clip_offset(row.range.end - 1, Bias::Left)
                .max(row.range.start)
        }
    };
    let (block_layouts, block_offsets) = blocks
        .blocks_in(first_screen_row..end_screen_row)
        .map(|(start, block)| {
            let layout = BlockLayout {
                id: block.id,
                rows: block.rows,
                rect: Rect {
                    x: text_left,
                    y: line_height * start as f32 - scroll.y,
                    width: text_width,
                    height: line_height * block.rows as f32,
                },
            };
            let above = block.above.min(row_count);
            let offset = above.checked_sub(1).map_or(0, row_end);
            (layout, (start..start + block.rows, offset))
        })
        .unzip();

    Frame {
        width,
        height,
        line_height,
        scroll,
        text_left,
        row_count,
        screen_rows,
        block_map: blocks.clone(),
        first_row,
        rows,
        blocks: block_layouts,
        block_offsets,
        active_lines,
        selections: selection_rects,
        cursors,
        line_numbers,
        fold_column: Rect {
            x: text_left - fold_column_width,
            y: 0.,
            width: fold_column_width,
            height,
        },
        fold_chevron: (em * FOLD_CHEVRON_EMS).min(line_height * FOLD_CHEVRON_LINE_FRACTION),
        fold_toggles,
        fold_markers,
        inlay: inlay_rect,
    }
}

/// Puts a run of `len` bytes in `style` at `at`, splitting the run it's in.
fn insert_run<S: Clone>(runs: &mut Vec<StyleRun<S>>, at: usize, len: usize, style: &S) {
    let mut start = 0;
    let mut ix = 0;
    while ix < runs.len() && start + runs[ix].len <= at {
        start += runs[ix].len;
        ix += 1;
    }
    let new = StyleRun {
        len,
        style: style.clone(),
    };
    if ix < runs.len() && start < at {
        let tail = StyleRun {
            len: start + runs[ix].len - at,
            style: runs[ix].style.clone(),
        };
        runs[ix].len = at - start;
        runs.insert(ix + 1, new);
        runs.insert(ix + 2, tail);
    } else {
        runs.insert(ix, new);
    }
}

/// Spans for the visible rows that are shaped from their start. Consecutive
/// rows (across a line break or a soft wrap) are queried together; a row cut
/// short ends the query, so the hidden rest of a long line isn't
/// highlighted. `rows` are the shown ranges with their buffer rows.
fn visible_spans<S>(
    text: &dyn TextSource,
    rows: &[(u32, Range<usize>)],
    highlights: &mut impl FnMut(Range<usize>) -> Vec<StyledSpan<S>>,
) -> Vec<StyledSpan<S>> {
    // Whether `end`, in `buffer_row`, is followed by `start`, past only the
    // line break, which is one byte or two.
    let across_line_break = |buffer_row: u32, end: usize, start: usize| {
        end == text.row_start(buffer_row) + text.row_len(buffer_row) as usize
            && buffer_row + 1 < text.row_count()
            && start == text.row_start(buffer_row + 1)
    };
    // Each range, with the buffer row it ends in.
    let mut ranges: Vec<(Range<usize>, u32)> = Vec::new();
    for (buffer_row, row) in rows {
        match ranges.last_mut() {
            Some((last, last_row))
                if row.start == last.end || across_line_break(*last_row, last.end, row.start) =>
            {
                last.end = row.end;
                *last_row = *buffer_row;
            }
            _ => ranges.push((row.clone(), *buffer_row)),
        }
    }
    ranges
        .into_iter()
        .flat_map(|(range, _)| highlights(range))
        .collect()
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
mod cluster_row_tests;
#[cfg(test)]
mod fold_column_tests;
#[cfg(test)]
mod fold_gutter_tests;
#[cfg(test)]
mod inlay_tests;
#[cfg(test)]
mod long_row_tests;
#[cfg(test)]
mod slice_cap_tests;
#[cfg(test)]
mod tests;
