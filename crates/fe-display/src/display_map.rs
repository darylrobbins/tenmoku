// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, file
// crates/base/src/input/editor/display_map/display_map.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: GPUI-free, follows `fe_buffer` snapshots and their
// edit log instead of being told about each change, maps byte offsets
// rather than points, and can show any read-only `TextSource`.

use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use fe_buffer::{BufferSnapshot, Text, Version};
use fe_text::TextSource;

use crate::{
    CharWidth,
    fold_map::{Fold, FoldMap},
    long_row::{SourceId, SourceKey},
    wrap_map::WrapMap,
};

/// A [`DisplayMap::wrap_pending`] budget that wraps in about 0.6 ms (release,
/// Apple Silicon, code-like rows).
pub const WRAP_SLICE_BYTES: usize = 128 * 1024;

/// Maps a buffer to the rows displayed: buffer rows are soft wrapped into
/// wrap rows, and folds hide some of those.
///
/// The map follows one buffer through [`DisplayMap::sync`], which re-wraps
/// only the rows edits touched. A new wrap width re-wraps the other rows in
/// slices through [`DisplayMap::wrap_pending`].
///
/// A map made with [`DisplayMap::read_only`] shows any [`TextSource`], such
/// as a file too big to load, one row per buffer row: it never soft wraps
/// or folds, and follows the source through [`DisplayMap::replace_text`].
pub struct DisplayMap {
    source: Source,
    tab_size: usize,
    wrap: WrapMap,
    folds: FoldMap,
    line_numbers: LineNumbers,
    scroll_off: u32,
}

/// The rows of margin autoscroll keeps by default.
pub const DEFAULT_SCROLL_OFF: u32 = 3;

/// The distance, in pixels, autoscroll keeps the caret from a window
/// `height` high's edges for `scroll_off` rows: the default keeps to a
/// third of the window, any other value to as far as leaves the caret's row
/// the middle one, so a huge value centres it.
pub fn scroll_margin(scroll_off: u32, line_height: f32, height: f32) -> f32 {
    let wanted = line_height * scroll_off as f32;
    match scroll_off == DEFAULT_SCROLL_OFF {
        true => wanted.min(height / 3.),
        false => wanted.min(((height - line_height) / 2.).max(0.)),
    }
}

/// Which numbers the gutter shows, as vim's 'number' and 'relativenumber'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineNumbers {
    /// Each row's number; with `relative`, only the caret's row's.
    pub number: bool,
    /// How many rows each row is from the primary caret's, counting a
    /// closed fold as one row and a wrapped row once.
    pub relative: bool,
}

impl Default for LineNumbers {
    /// Each row's number.
    fn default() -> Self {
        Self {
            number: true,
            relative: false,
        }
    }
}

/// The text a [`DisplayMap`] shows.
enum Source {
    Buffer(BufferSnapshot),
    /// A read-only source, with the id [`SourceKey`]s tell it apart by.
    ReadOnly {
        text: Arc<dyn TextSource>,
        id: SourceId,
    },
}

impl Source {
    fn text(&self) -> &dyn TextSource {
        match self {
            Source::Buffer(snapshot) => snapshot.text(),
            Source::ReadOnly { text, .. } => &**text,
        }
    }
}

/// One displayed row: all or part of a buffer row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    pub buffer_row: u32,
    /// The buffer offsets shown.
    pub range: Range<usize>,
    /// Starts its buffer row; continuation rows of a wrapped row don't.
    pub first: bool,
    /// Ends its buffer row, rather than at a soft wrap.
    pub last: bool,
    /// The buffer rows after this one are folded away.
    pub folded: bool,
}

impl DisplayMap {
    /// Shows every buffer row of `snapshot` as one row, unwrapped and
    /// unfolded.
    pub fn new(snapshot: &BufferSnapshot, tab_size: usize) -> Self {
        Self::with_source(Source::Buffer(snapshot.clone()), tab_size)
    }

    /// Shows every row of `text` as one row. The map never soft wraps or
    /// folds, and [`DisplayMap::replace_text`] moves it on to a later
    /// snapshot of the same source.
    pub fn read_only(text: Arc<dyn TextSource>, tab_size: usize) -> Self {
        let id = SourceId::new();
        Self::with_source(Source::ReadOnly { text, id }, tab_size)
    }

    fn with_source(source: Source, tab_size: usize) -> Self {
        let mut map = Self {
            wrap: WrapMap::new(source.text(), tab_size),
            source,
            tab_size: tab_size.max(1),
            folds: FoldMap::default(),
            line_numbers: LineNumbers::default(),
            scroll_off: DEFAULT_SCROLL_OFF,
        };
        map.rebuild_folds();
        map
    }

    pub fn text(&self) -> &dyn TextSource {
        self.source.text()
    }

    /// The buffer snapshot the rows are of, or `None` for a read-only map.
    pub fn snapshot(&self) -> Option<&BufferSnapshot> {
        match &self.source {
            Source::Buffer(snapshot) => Some(snapshot),
            Source::ReadOnly { .. } => None,
        }
    }

    /// Which text the rows are of, for [`crate::LongRows`].
    pub fn source_key(&self) -> SourceKey {
        match &self.source {
            Source::Buffer(snapshot) => snapshot.into(),
            Source::ReadOnly { text, id } => SourceKey::read_only(*id, &**text),
        }
    }

    fn is_read_only(&self) -> bool {
        matches!(self.source, Source::ReadOnly { .. })
    }

    /// Moves a read-only map on to `text`, a later snapshot of the same
    /// source: the rows it had, but for its last, which may have grown, and
    /// perhaps more after them. The rows shown before keep their layout.
    /// A caller bug on a buffer's map, which panics in debug builds and is
    /// ignored otherwise.
    pub fn replace_text(&mut self, text: Arc<dyn TextSource>) {
        let Source::ReadOnly { text: ours, .. } = &mut self.source else {
            debug_assert!(false, "replace_text on a buffer's DisplayMap");
            return;
        };
        debug_assert!(text.row_count() >= ours.row_count());
        *ours = text;
        self.wrap.reset(self.source.text());
        self.rebuild_folds();
    }

    /// Catches up with `snapshot`, a later version of the same buffer,
    /// re-wrapping the rows edited since. Folds edits touched are dropped.
    /// Another buffer, or an earlier version, shows unwrapped and unfolded,
    /// with every row pending a wrap.
    ///
    /// A read-only map has no buffer to follow: syncing one is a caller bug,
    /// which panics in debug builds and is ignored otherwise.
    pub fn sync(&mut self, snapshot: &BufferSnapshot, widths: &mut dyn CharWidth) {
        let Source::Buffer(ours) = &self.source else {
            debug_assert!(false, "sync on a read-only DisplayMap");
            return;
        };
        if snapshot.id() == ours.id() && snapshot.version() == ours.version() {
            return;
        }
        if snapshot.id() != ours.id() || snapshot.version() < ours.version() {
            // Another buffer, or history we can't follow.
            self.source = Source::Buffer(snapshot.clone());
            self.folds.unfold_all();
            self.wrap.reset(snapshot.text());
            self.rebuild_folds();
            return;
        }
        let mut wrap_edits = Vec::new();
        for edit in row_edits(ours.text(), snapshot, ours.version()) {
            wrap_edits.push(self.wrap.splice(
                snapshot.text(),
                edit.start as usize,
                edit.old_rows as usize,
                edit.new_rows as usize,
                widths,
            ));
            self.folds.edit(edit.start, edit.old_rows, edit.new_rows);
        }
        self.source = Source::Buffer(snapshot.clone());
        self.folds
            .sync(&self.wrap, self.text().row_count(), &wrap_edits);
    }

    /// Which numbers the gutter shows; [`LineNumbers::default`] until set.
    pub fn line_numbers(&self) -> LineNumbers {
        self.line_numbers
    }

    pub fn set_line_numbers(&mut self, line_numbers: LineNumbers) {
        self.line_numbers = line_numbers;
    }

    /// How many rows autoscroll keeps between the caret and the window's
    /// edge, vim's 'scrolloff'; [`DEFAULT_SCROLL_OFF`] until set.
    pub fn scroll_off(&self) -> u32 {
        self.scroll_off
    }

    pub fn set_scroll_off(&mut self, scroll_off: u32) {
        self.scroll_off = scroll_off;
    }

    /// `buffer_row`'s line among those not folded away, a closed fold
    /// counting as one: a hidden row is its fold's header's line. What
    /// relative line numbers count in.
    pub fn visible_line(&self, buffer_row: u32) -> u32 {
        // A hidden row counts itself and the hidden rows before it in its
        // fold, which brings it back to the header's line.
        buffer_row - self.folds.hidden_buffer_rows_through(buffer_row)
    }

    pub fn wrap_width(&self) -> Option<f32> {
        self.wrap.width()
    }

    /// Wraps rows wider than `width`, as measured by `widths`, or with
    /// `None` stops wrapping. A new width wraps nothing yet: every row is
    /// pending, shown as it was, until [`DisplayMap::wrap_pending`] gets to
    /// it. A read-only map stays unwrapped.
    pub fn set_wrap_width(&mut self, width: Option<f32>, widths: &mut dyn CharWidth) {
        let width = width.filter(|_| !self.is_read_only());
        if width != self.wrap.width() {
            self.wrap
                .rebuild(self.source.text(), width, self.tab_size, widths);
            self.rebuild_folds();
        }
    }

    pub fn set_tab_size(&mut self, tab_size: usize, widths: &mut dyn CharWidth) {
        let tab_size = tab_size.max(1);
        if tab_size != self.tab_size {
            self.tab_size = tab_size;
            self.rewrap(widths);
        }
    }

    /// Marks every row pending a wrap, for when `widths` measures
    /// differently, such as after a font change.
    pub fn rewrap(&mut self, widths: &mut dyn CharWidth) {
        self.wrap
            .rebuild(self.source.text(), self.wrap.width(), self.tab_size, widths);
        self.rebuild_folds();
    }

    /// Whether rows are still to be wrapped at the current width, tab size
    /// and font.
    pub fn wrap_is_pending(&self) -> bool {
        self.wrap.is_pending()
    }

    /// Wraps pending rows, those from buffer row `around` onward first, then
    /// those before it, nearest first, until it has wrapped `budget_bytes`
    /// of rows (at least one row). Returns whether rows are still pending.
    /// [`WRAP_SLICE_BYTES`] keeps a call well inside a frame.
    pub fn wrap_pending(
        &mut self,
        around: u32,
        budget_bytes: usize,
        widths: &mut dyn CharWidth,
    ) -> bool {
        if !self.wrap.is_pending() {
            return false;
        }
        let pending = self
            .wrap
            .wrap_pending(self.source.text(), around, budget_bytes, widths);
        self.rebuild_folds();
        pending
    }

    fn rebuild_folds(&mut self) {
        self.folds.rebuild(&self.wrap, self.text().row_count());
    }

    pub fn row_count(&self) -> u32 {
        self.folds.display_row_count() as u32
    }

    /// Display row `row`, clamped to the last.
    pub fn row(&self, row: u32) -> DisplayRow {
        let row = (row as usize).min(self.folds.display_row_count().saturating_sub(1));
        let wrap_row = self.folds.to_wrap(row).unwrap_or(0);
        let segment = self.wrap.segment(self.text(), wrap_row);
        let start = self.text().row_start(segment.buffer_row);
        let last = segment.index + 1 == segment.segments;
        DisplayRow {
            buffer_row: segment.buffer_row,
            range: start + segment.range.start..start + segment.range.end,
            first: segment.index == 0,
            last,
            folded: last && self.folds.hidden_run(wrap_row + 1).is_some(),
        }
    }

    /// The display row showing `offset`. An offset at a soft wrap shows at
    /// the start of the row after it; a folded one, on the fold's header.
    pub fn row_for_offset(&self, offset: usize) -> u32 {
        let point = self.text().offset_to_point(offset);
        let wrap_row = self.wrap.row_for(point.row as usize, point.column as usize);
        self.folds.nearest_display_row(wrap_row) as u32
    }

    /// The first display row of `buffer_row`, or if it's folded, its
    /// fold's header.
    pub fn row_for_buffer_row(&self, buffer_row: u32) -> u32 {
        let wrap_row = self.wrap.first_row(buffer_row as usize);
        self.folds.nearest_display_row(wrap_row) as u32
    }

    /// Folds `rows` behind their first. Returns false if there's only one,
    /// or the map is read-only.
    pub fn fold(&mut self, rows: RangeInclusive<u32>) -> bool {
        if self.is_read_only() {
            return false;
        }
        let last = self.text().row_count() - 1;
        let folded = self.folds.fold(*rows.start()..=(*rows.end()).min(last));
        self.rebuild_folds();
        folded
    }

    /// Folds each of `folds` as [`DisplayMap::fold`] does, rebuilding once.
    pub fn fold_many(&mut self, folds: impl IntoIterator<Item = RangeInclusive<u32>>) {
        if self.is_read_only() {
            return;
        }
        let last = self.text().row_count() - 1;
        let folds = folds
            .into_iter()
            .map(|rows| *rows.start()..=(*rows.end()).min(last));
        self.folds.fold_many(folds);
        self.rebuild_folds();
    }

    /// Unfolds the folds whose header or hidden rows overlap `buffer_rows`.
    pub fn unfold(&mut self, buffer_rows: RangeInclusive<u32>) -> bool {
        let unfolded = self.folds.unfold(buffer_rows);
        self.rebuild_folds();
        unfolded
    }

    pub fn unfold_all(&mut self) {
        self.folds.unfold_all();
        self.rebuild_folds();
    }

    /// Sorted by first row; folds may nest.
    pub fn folds(&self) -> &[Fold] {
        self.folds.folds()
    }

    /// The buffer rows hidden together with `buffer_row`, if it's folded.
    pub fn hidden_rows(&self, buffer_row: u32) -> Option<Range<u32>> {
        let run = self
            .folds
            .hidden_run(self.wrap.first_row(buffer_row as usize))?;
        let first = self.wrap.segment(self.text(), run.start).buffer_row;
        let end = if run.end < self.wrap.row_count() {
            self.wrap.segment(self.text(), run.end).buffer_row
        } else {
            self.text().row_count()
        };
        Some(first..end)
    }
}

/// Buffer rows `start..start + old_rows` replaced by `new_rows` rows.
#[derive(Debug, PartialEq, Eq)]
struct RowEdit {
    start: u32,
    old_rows: u32,
    new_rows: u32,
}

impl RowEdit {
    /// The rows a replacement of `old` (in `before`) by `new` (in `after`)
    /// touched. Rows the edit only ends at the start of are untouched, so
    /// inserting or deleting whole lines leaves the rows around them be.
    fn new(before: &Text, old: Range<usize>, after: &Text, new: Range<usize>) -> Self {
        let (old_start, old_end) = (
            before.offset_to_point(old.start),
            before.offset_to_point(old.end),
        );
        let (new_start, new_end) = (
            after.offset_to_point(new.start),
            after.offset_to_point(new.end),
        );
        let whole_rows = [old_start, old_end, new_start, new_end]
            .iter()
            .all(|point| point.column == 0);
        let touched = u32::from(!whole_rows);
        RowEdit {
            start: new_start.row,
            old_rows: old_end.row - old_start.row + touched,
            new_rows: new_end.row - new_start.row + touched,
        }
    }
}

/// The rows changed from version `from` (whose text is `old`) to `new`, in
/// order, each starting in the coordinates left by those before it.
///
/// Rows are only known for the first and last texts, so several batches
/// merge into one edit spanning them all, as for tree-sitter in `fe-syntax`.
fn row_edits(old: &Text, new: &BufferSnapshot, from: Version) -> Vec<RowEdit> {
    let batches: Vec<_> = new.edits_since(from).collect();
    if let [batch] = batches.as_slice() {
        return batch
            .iter()
            .map(|edit| RowEdit::new(old, edit.old.clone(), new.text(), edit.new.clone()))
            .collect();
    }
    // Start, end in `old`, and end in the text after the batches so far.
    let mut hull: Option<(usize, usize, usize)> = None;
    for batch in batches {
        let (Some(first), Some(last)) = (batch.first(), batch.last()) else {
            continue;
        };
        hull = Some(match hull {
            None => (first.old.start, last.old.end, last.new.end),
            Some((start, old_end, current_end)) => {
                // `current_end` and the batch's old range share coordinates.
                let end = current_end.max(last.old.end);
                (
                    start.min(first.old.start),
                    old_end + (end - current_end),
                    last.new.end + (end - last.old.end),
                )
            }
        });
    }
    hull.map(|(start, old_end, new_end)| {
        RowEdit::new(old, start..old_end, new.text(), start..new_end)
    })
    .into_iter()
    .collect()
}

#[cfg(test)]
mod pending_wrap_tests;
#[cfg(test)]
mod tests;
