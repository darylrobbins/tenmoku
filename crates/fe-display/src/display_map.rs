// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, file
// crates/base/src/input/editor/display_map/display_map.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: GPUI-free, follows `fe_buffer` snapshots and their
// edit log instead of being told about each change, and maps byte offsets
// rather than points.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{BufferSnapshot, Text, Version};

use crate::{
    CharWidth,
    fold_map::{Fold, FoldMap},
    wrap_map::WrapMap,
};

/// Maps a buffer to the rows displayed: buffer rows are soft wrapped into
/// wrap rows, and folds hide some of those.
///
/// The map follows one buffer through [`DisplayMap::sync`], which re-wraps
/// only the rows edits touched.
pub struct DisplayMap {
    snapshot: BufferSnapshot,
    tab_size: usize,
    wrap: WrapMap,
    folds: FoldMap,
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
        let mut map = Self {
            snapshot: snapshot.clone(),
            tab_size: tab_size.max(1),
            wrap: WrapMap::new(snapshot.text(), tab_size),
            folds: FoldMap::default(),
        };
        map.rebuild_folds();
        map
    }

    pub fn text(&self) -> &Text {
        self.snapshot.text()
    }

    /// Catches up with `snapshot`, a later version of the same buffer,
    /// re-wrapping the rows edited since. Folds edits touched are dropped.
    pub fn sync(&mut self, snapshot: &BufferSnapshot, widths: &mut dyn CharWidth) {
        let ours = &self.snapshot;
        if snapshot.id() == ours.id() && snapshot.version() == ours.version() {
            return;
        }
        if snapshot.id() != ours.id() || snapshot.version() < ours.version() {
            // Another buffer, or history we can't follow.
            self.snapshot = snapshot.clone();
            self.folds.unfold_all();
            self.rewrap(widths);
            return;
        }
        for edit in row_edits(ours.text(), snapshot, ours.version()) {
            self.wrap.splice(
                snapshot.text(),
                edit.start as usize,
                edit.old_rows as usize,
                edit.new_rows as usize,
                widths,
            );
            self.folds.edit(edit.start, edit.old_rows, edit.new_rows);
        }
        self.snapshot = snapshot.clone();
        self.rebuild_folds();
    }

    pub fn wrap_width(&self) -> Option<f32> {
        self.wrap.width()
    }

    /// Wraps rows wider than `width`, as measured by `widths`, or with
    /// `None` stops wrapping. Re-wraps every row if the width changed.
    pub fn set_wrap_width(&mut self, width: Option<f32>, widths: &mut dyn CharWidth) {
        if width != self.wrap.width() {
            self.wrap
                .rebuild(self.snapshot.text(), width, self.tab_size, widths);
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

    /// Re-wraps every row, for when `widths` measures differently, such as
    /// after a font change.
    pub fn rewrap(&mut self, widths: &mut dyn CharWidth) {
        self.wrap.rebuild(
            self.snapshot.text(),
            self.wrap.width(),
            self.tab_size,
            widths,
        );
        self.rebuild_folds();
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

    /// Folds `rows` behind their first. Returns false if there's only one.
    pub fn fold(&mut self, rows: RangeInclusive<u32>) -> bool {
        let last = self.text().row_count() - 1;
        let folded = self.folds.fold(*rows.start()..=(*rows.end()).min(last));
        self.rebuild_folds();
        folded
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
mod tests;
