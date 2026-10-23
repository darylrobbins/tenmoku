//! What the minimap marks besides the code, as VS Code's does: selections,
//! find matches, errors and warnings at their rows and columns, under the
//! tiles so the code's shape shows over them, and git's changed rows in a
//! lane at the minimap's inner edge, over the tiles.
//!
//! They change at every caret move, so they're built each frame from their
//! sources, for the rows the minimap shows only, and never go into a tile.
//! Rows are drawn as the tiles draw them ([`Sampling`]): a mark on a row
//! the tiles skip is drawn on the row drawn in its place, and a selection's
//! rows between its first and last only where the tiles draw a row, so a
//! long file's marks are as many as the minimap's pixels, not its rows.

use std::ops::Range;

use fe_buffer::Bias;
use fe_display::{
    DisplayMap,
    layout::Rect,
    minimap::{MinimapLayout, row_runs},
};
use fe_editor::{diagnostics::Severity, row_changes::RowChangeKind};
use gpui::Hsla;

use super::Sampling;
use crate::EditorView;

/// The git lane's width, in logical pixels: a few, as VS Code's minimap
/// gutter is.
pub(crate) const LANE_WIDTH: f32 = 2.;

/// The marks one frame draws, in the frame's coordinates.
#[derive(Default)]
pub(crate) struct Marks {
    /// Under the tiles, in paint order: selections, find matches, warnings
    /// and errors.
    pub(crate) under: Vec<(Rect, Hsla)>,
    /// The git lane's bars and deletion marks, over the tiles.
    pub(crate) lane: Vec<(Rect, Hsla)>,
}

/// The marks on the rows `geometry` shows, `sampling` the rows the tiles
/// draw, `column_width` a column's width and `max_column` the last drawn.
pub(crate) fn layout(
    geometry: &MinimapLayout,
    sampling: Sampling,
    column_width: f32,
    max_column: u32,
    view: &EditorView,
) -> Marks {
    let mut marks = Marks::default();
    let display = view.model.display();
    if geometry.first_row >= geometry.end_row.min(display.row_count()) {
        return marks;
    }
    let shown = Shown::new(geometry, display, view, sampling, column_width, max_column);
    let theme = &view.theme;
    let offsets = shown.offsets.clone();
    let over = |range: &Range<usize>| range.end >= offsets.start && range.start <= offsets.end;

    let selections = view.model.selections();
    let first = selections.partition_point(|selection| selection.range.end < offsets.start);
    for selection in (selections[first..].iter())
        .take_while(|selection| selection.range.start <= offsets.end)
        .filter(|selection| !selection.range.is_empty())
    {
        shown.span(
            &selection.range,
            false,
            theme.minimap_selection,
            &mut marks.under,
        );
    }

    if let Some(matches) = view.find_counted() {
        let first = matches.partition_point(|found| found.end < offsets.start);
        for found in matches[first..]
            .iter()
            .take_while(|found| found.start <= offsets.end)
        {
            shown.span(found, true, theme.minimap_find_match, &mut marks.under);
        }
    }

    let diagnostics = view.model.diagnostics();
    for (severity, color) in [
        (Severity::Warning, theme.minimap_warning),
        (Severity::Error, theme.minimap_error),
    ] {
        for entry in
            (diagnostics.iter()).filter(|entry| entry.severity == severity && over(&entry.range))
        {
            shown.span(&entry.range, true, color, &mut marks.under);
        }
    }

    shown.lane(view, &mut marks.lane);
    marks
}

/// The rows the minimap shows, and how it draws them.
struct Shown<'a> {
    geometry: &'a MinimapLayout,
    display: &'a DisplayMap,
    view: &'a EditorView,
    sampling: Sampling,
    column_width: f32,
    max_column: u32,
    /// The display rows shown.
    rows: Range<u32>,
    /// The text they show, what their folds hide included.
    offsets: Range<usize>,
}

impl<'a> Shown<'a> {
    fn new(
        geometry: &'a MinimapLayout,
        display: &'a DisplayMap,
        view: &'a EditorView,
        sampling: Sampling,
        column_width: f32,
        max_column: u32,
    ) -> Self {
        let rows = geometry.first_row..geometry.end_row.min(display.row_count());
        let start = display.row(rows.start).range.start;
        let end = match rows.end < display.row_count() {
            true => display.row(rows.end).range.start,
            false => view.model.text().len(),
        };
        Self {
            geometry,
            display,
            view,
            sampling,
            column_width,
            max_column,
            rows,
            offsets: start..end,
        }
    }

    /// The top of the row the tiles draw in display row `row`'s place.
    fn top(&self, row: u32) -> f32 {
        let stride = self.sampling.stride;
        self.geometry.row_y(row / stride * stride)
    }

    /// A drawn row's height.
    fn height(&self) -> f32 {
        self.sampling.stride as f32 * self.geometry.row_height
    }

    /// The minimap columns `bytes`, within display row `row`, cover, as
    /// [`row_runs`] counts them, clipped at the last drawn.
    fn columns(&self, row: u32, bytes: Range<usize>) -> Range<u32> {
        let text = self.view.model.text();
        let range = self.display.row(row).range;
        // A char is a column at least and 4 bytes at most: past this the
        // columns are past the last drawn.
        let reach = text.clip_offset(
            range.end.min(range.start + 4 * self.max_column as usize),
            Bias::Left,
        );
        let clip = |offset: usize| text.clip_offset(offset.clamp(range.start, reach), Bias::Left);
        let (start, end) = (clip(bytes.start), clip(bytes.end));
        let row_text = text.slice_to_string(range.start..end);
        let tab_size = self.view.model.tab_size();
        let column = |byte: usize| {
            // The column a char at `byte` would start at: the last of a run
            // ending in one more.
            let mut before = row_text[..byte].to_owned();
            before.push('x');
            let runs = row_runs(&before, tab_size, &[], (), u32::MAX);
            let column = runs.last().map_or(0, |run| run.columns.end - 1);
            column.min(self.max_column)
        };
        column(start - range.start)..column(end - range.start)
    }

    /// Marks `range` in `color` on each row it covers that the tiles draw,
    /// its first and last rows always: from its start, to its end, and the
    /// rows between to their last column; an empty range one column wide if
    /// `one_column`, or nothing.
    fn span(
        &self,
        range: &Range<usize>,
        one_column: bool,
        color: Hsla,
        out: &mut Vec<(Rect, Hsla)>,
    ) {
        let first = self.display.row_for_offset(range.start);
        let last = self.display.row_for_offset(range.end).max(first);
        let stride = self.sampling.stride;
        let from = first.max(self.rows.start);
        let to = last.min(self.rows.end - 1);
        if from > to {
            return;
        }
        // The rows between that the tiles draw, as many as are shown.
        let between = (from.max(first + 1).div_ceil(stride) * stride..(to + 1).min(last))
            .step_by(stride as usize);
        let rows = (self.rows.contains(&first).then_some(first))
            .into_iter()
            .chain(between)
            .chain((last != first && self.rows.contains(&last)).then_some(last));
        for row in rows {
            let mut columns = self.columns(row, range.clone());
            // Past the last column drawn, an empty range is as hidden as
            // any other.
            if one_column && range.is_empty() && columns.start < self.max_column {
                columns = columns.start..columns.start + 1;
            }
            if columns.is_empty() {
                continue;
            }
            let rect = Rect {
                x: self.geometry.x.start + columns.start as f32 * self.column_width,
                y: self.top(row),
                width: columns.len() as f32 * self.column_width,
                height: self.height(),
            };
            out.push((rect, color));
        }
    }

    /// The git lane's marks on the rows shown: a bar over each added or
    /// modified row, bars of one colour that meet merged, and a short mark
    /// over the edge where rows were deleted.
    fn lane(&self, view: &EditorView, out: &mut Vec<(Rect, Hsla)>) {
        let changes = view.model.row_changes();
        if changes.is_empty() {
            return;
        }
        let (text, theme) = (view.model.text(), &view.theme);
        let first_row = text.offset_to_point(self.offsets.start).row;
        // Shown to the end, rows deleted from the end are below the last
        // row: at the row count.
        let last_row = match self.offsets.end >= text.len() {
            true => text.row_count(),
            false => text.offset_to_point(self.offsets.end).row,
        };
        let x = self.geometry.x.start;
        let height = self.height();
        let first = changes.partition_point(|change| change.rows.end < first_row);
        for change in changes[first..]
            .iter()
            .take_while(|change| change.rows.start <= last_row)
        {
            let mark = match change.kind {
                RowChangeKind::Deleted => {
                    // The edge above the row after them, or below the last.
                    let below = match change.rows.start < text.row_count() {
                        true => self.display.row_for_buffer_row(change.rows.start),
                        false => self.display.row_count(),
                    };
                    if below < self.rows.start || below > self.rows.end {
                        continue;
                    }
                    let edge = match below < self.display.row_count() {
                        true => self.top(below),
                        false => self.geometry.row_y(below),
                    };
                    let rect = Rect {
                        x,
                        y: edge - height / 2.,
                        width: LANE_WIDTH,
                        height,
                    };
                    out.push((rect, theme.minimap_gutter_deleted));
                    continue;
                }
                kind => {
                    if change.rows.is_empty() {
                        continue;
                    }
                    let start = self.display.row_for_buffer_row(change.rows.start);
                    let last = change.rows.end - 1;
                    let end_offset = text.row_start(last) + text.row_len(last) as usize;
                    let end = self.display.row_for_offset(end_offset);
                    let (start, end) = (start.max(self.rows.start), end.min(self.rows.end - 1));
                    if start > end {
                        continue;
                    }
                    let color = match kind {
                        RowChangeKind::Added => theme.minimap_gutter_added,
                        _ => theme.minimap_gutter_modified,
                    };
                    let top = self.top(start);
                    let rect = Rect {
                        x,
                        y: top,
                        width: LANE_WIDTH,
                        height: self.top(end) + height - top,
                    };
                    (rect, color)
                }
            };
            let (rect, color) = mark;
            match out.last_mut() {
                Some((last, last_color))
                    if *last_color == color && rect.y <= last.y + last.height =>
                {
                    last.height = last.height.max(rect.y + rect.height - last.y);
                }
                _ => out.push(mark),
            }
        }
    }
}

#[cfg(test)]
#[path = "minimap_marks_tests.rs"]
mod tests;
