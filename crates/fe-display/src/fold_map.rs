// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, files
// crates/base/src/input/editor/display_map/{fold_map,folding}.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: a fold hides every row after its header through its
// last row (gpui-kit keeps the last row visible), folds are set directly
// rather than chosen from candidates, edits to a fold's header row keep it,
// and the projection is rebuilt eagerly.

//! Folding: wrap rows ↔ display rows.
//!
//! The rows folds hide are kept as sorted, merged runs of wrap rows, each
//! with the number of rows hidden before it, so rebuilding costs the number
//! of folds and mapping either way is a binary search. An edit re-resolves
//! only the runs it touches and shifts the rest.

use std::{
    iter, mem,
    ops::{Range, RangeInclusive},
};

use crate::wrap_map::{Splice, WrapEdit, WrapMap};

/// Buffer rows `start + 1..=end` hidden behind header row `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fold {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HiddenRows {
    wrap_rows: Range<usize>,
    /// How many wrap rows the runs before this one hide.
    hidden_before: usize,
    /// The buffer rows `wrap_rows` are of.
    buffer_rows: Range<u32>,
}

impl HiddenRows {
    /// The display row of the first visible wrap row after this run.
    fn display_start(&self) -> usize {
        self.wrap_rows.start - self.hidden_before
    }
}

#[derive(Default)]
pub(crate) struct FoldMap {
    /// Sorted by start; may nest or overlap.
    folds: Vec<Fold>,
    /// Sorted, disjoint and non-adjacent.
    hidden: Vec<HiddenRows>,
    total_hidden: usize,
    wrap_row_count: usize,
}

impl FoldMap {
    pub(crate) fn folds(&self) -> &[Fold] {
        &self.folds
    }

    pub(crate) fn display_row_count(&self) -> usize {
        self.wrap_row_count - self.total_hidden
    }

    /// How many wrap rows the runs before `index` hide.
    fn hidden_before(&self, index: usize) -> usize {
        self.hidden
            .get(index)
            .map_or(self.total_hidden, |run| run.hidden_before)
    }

    /// `None` if `wrap_row` is hidden or out of range.
    pub(crate) fn to_display(&self, wrap_row: usize) -> Option<usize> {
        if wrap_row >= self.wrap_row_count {
            return None;
        }
        let index = self
            .hidden
            .partition_point(|run| run.wrap_rows.end <= wrap_row);
        if self
            .hidden
            .get(index)
            .is_some_and(|run| run.wrap_rows.start <= wrap_row)
        {
            return None;
        }
        Some(wrap_row - self.hidden_before(index))
    }

    /// `None` past the last display row.
    pub(crate) fn to_wrap(&self, display_row: usize) -> Option<usize> {
        if display_row >= self.display_row_count() {
            return None;
        }
        let index = self
            .hidden
            .partition_point(|run| run.display_start() <= display_row);
        Some(display_row + self.hidden_before(index))
    }

    /// The display row of `wrap_row`, or for a hidden row the visible row
    /// before it.
    pub(crate) fn nearest_display_row(&self, wrap_row: usize) -> usize {
        if let Some(display_row) = self.to_display(wrap_row) {
            return display_row;
        }
        let visible_before = if wrap_row >= self.wrap_row_count {
            self.display_row_count()
        } else {
            let index = self
                .hidden
                .partition_point(|run| run.wrap_rows.end <= wrap_row);
            self.hidden
                .get(index)
                .map_or(self.display_row_count(), HiddenRows::display_start)
        };
        visible_before.saturating_sub(1)
    }

    /// How many of buffer rows `0..=buffer_row` are hidden.
    pub(crate) fn hidden_buffer_rows_through(&self, buffer_row: u32) -> u32 {
        self.hidden
            .iter()
            .take_while(|run| run.buffer_rows.start <= buffer_row)
            .map(|run| run.buffer_rows.end.min(buffer_row + 1) - run.buffer_rows.start)
            .sum()
    }

    /// The run of hidden wrap rows containing `wrap_row`.
    pub(crate) fn hidden_run(&self, wrap_row: usize) -> Option<Range<usize>> {
        let index = self
            .hidden
            .partition_point(|run| run.wrap_rows.end <= wrap_row);
        let run = self.hidden.get(index)?;
        (run.wrap_rows.start <= wrap_row).then(|| run.wrap_rows.clone())
    }

    /// Folds `rows` behind their first row. Returns false for a single row.
    pub(crate) fn fold(&mut self, rows: RangeInclusive<u32>) -> bool {
        let fold = Fold {
            start: *rows.start(),
            end: *rows.end(),
        };
        if fold.end <= fold.start {
            return false;
        }
        if let Err(index) = self
            .folds
            .binary_search_by_key(&(fold.start, fold.end), |f| (f.start, f.end))
        {
            self.folds.insert(index, fold);
        }
        true
    }

    /// Folds each of `folds` as [`FoldMap::fold`] does, sorting once.
    pub(crate) fn fold_many(&mut self, folds: impl IntoIterator<Item = RangeInclusive<u32>>) {
        let folds = folds.into_iter().map(|rows| Fold {
            start: *rows.start(),
            end: *rows.end(),
        });
        self.folds
            .extend(folds.filter(|fold| fold.end > fold.start));
        self.folds
            .sort_unstable_by_key(|fold| (fold.start, fold.end));
        self.folds.dedup();
    }

    /// Removes the folds whose header or hidden rows overlap `rows`.
    pub(crate) fn unfold(&mut self, rows: RangeInclusive<u32>) -> bool {
        let before = self.folds.len();
        self.folds
            .retain(|fold| fold.end < *rows.start() || fold.start > *rows.end());
        self.folds.len() != before
    }

    pub(crate) fn unfold_all(&mut self) {
        self.folds.clear();
    }

    /// Follows an edit that replaced buffer rows `start..start + old_rows`
    /// with `new_rows` rows (inserting before `start` if `old_rows` is 0).
    /// Folds whose hidden rows it touched are dropped, as are folds whose
    /// header it split or joined; later folds shift.
    pub(crate) fn edit(&mut self, start: u32, old_rows: u32, new_rows: u32) {
        let old_end = start + old_rows;
        let delta = new_rows as i64 - old_rows as i64;
        self.folds.retain_mut(|fold| {
            let touches_hidden = start <= fold.end && old_end.max(start + 1) > fold.start + 1;
            let touches_header = start <= fold.start && old_end > fold.start;
            if touches_hidden || (touches_header && delta != 0) {
                return false;
            }
            if fold.start >= old_end {
                fold.start = (fold.start as i64 + delta) as u32;
                fold.end = (fold.end as i64 + delta) as u32;
            }
            true
        });
    }

    /// Recomputes the hidden runs for `wrap_map`'s rows.
    pub(crate) fn rebuild(&mut self, wrap_map: &WrapMap, buffer_rows: u32) {
        self.folds.retain(|fold| fold.end < buffer_rows);
        let folds = mem::take(&mut self.folds);
        let runs = resolve(wrap_map, buffer_runs(&folds));
        self.set_sorted_hidden_rows(wrap_map.row_count(), runs);
        self.folds = folds;
    }

    /// Follows `edits`, which `wrap_map` has made since the last sync or
    /// rebuild, each in the coordinates the ones before it left, after
    /// [`FoldMap::edit`] has followed each of them. The same as
    /// [`FoldMap::rebuild`], but only runs the edits touch, and the folds in
    /// them, are found in `wrap_map` again: the others shift.
    pub(crate) fn sync(&mut self, wrap_map: &WrapMap, buffer_rows: u32, edits: &[WrapEdit]) {
        let fold_count = self.folds.len();
        self.folds.retain(|fold| fold.end < buffer_rows);
        if self.folds.len() != fold_count {
            self.rebuild(wrap_map, buffer_rows);
            return;
        }
        self.wrap_row_count = wrap_map.row_count();
        let edits = compose(edits);
        // The runs to find again, and what replaces them, in order.
        let mut windows: Vec<Window> = Vec::new();
        let mut fresh = Vec::new();
        let mut window_runs = Vec::new();
        let mut shift = Shift::default();
        let mut edits = edits.iter().peekable();
        while let Some(edit) = edits.next() {
            // The old buffer rows to find again: the edits, and the runs
            // overlapping or touching them, transitively.
            let (mut lo, mut hi) = (edit.old.start, edit.old.end);
            let mut group = Shift::default();
            group.add(edit);
            let next = windows.last().map_or(0, |window| window.runs.end);
            let first = next + self.hidden[next..].partition_point(|run| run.buffer_rows.end < lo);
            let mut end = first;
            loop {
                while let Some(run) = self.hidden.get(end)
                    && run.buffer_rows.start <= hi
                {
                    lo = lo.min(run.buffer_rows.start);
                    hi = hi.max(run.buffer_rows.end);
                    end += 1;
                }
                let Some(edit) = edits.next_if(|edit| edit.old.start <= hi) else {
                    break;
                };
                hi = hi.max(edit.old.end);
                group.add(edit);
            }

            let hidden_before = self.hidden_before(first).wrapping_add_signed(shift.hidden);
            // Both bounds are outside the edits, where rows only shift.
            let lo = lo.wrapping_add_signed(shift.buffer as i32);
            let hi = hi.wrapping_add_signed((shift.buffer + group.buffer) as i32);
            let folds = &self.folds[self.folds.partition_point(|fold| fold.start + 1 < lo)
                ..self.folds.partition_point(|fold| fold.start < hi)];
            let mut total_hidden = hidden_before;
            for (buffer_rows, wrap_rows) in resolve(wrap_map, buffer_runs(folds)) {
                push_run(
                    &mut window_runs,
                    &mut total_hidden,
                    self.wrap_row_count,
                    buffer_rows,
                    wrap_rows,
                );
            }
            let old_hidden: usize = self.hidden[first..end]
                .iter()
                .map(|run| run.wrap_rows.len())
                .sum();
            shift.buffer += group.buffer;
            shift.wrap += group.wrap;
            shift.hidden += (total_hidden - hidden_before) as isize - old_hidden as isize;
            windows.push(Window {
                runs: first..end,
                fresh: fresh.len()..fresh.len() + window_runs.len(),
                shift_after: shift,
            });
            fresh.append(&mut window_runs);
        }
        self.apply_windows(&windows, fresh);
        debug_assert!(self.hidden.windows(2).all(|pair| {
            pair[0].wrap_rows.end < pair[1].wrap_rows.start
                && pair[0].buffer_rows.end < pair[1].buffer_rows.start
        }));
        self.total_hidden = self
            .hidden
            .last()
            .map_or(0, |run| run.hidden_before + run.wrap_rows.len());
    }

    /// Replaces each of `windows`' runs with its runs of `fresh`, shifting
    /// the runs after it, in one pass over the runs: in place if no window
    /// changes the number of runs, otherwise copying each run once.
    fn apply_windows(&mut self, windows: &[Window], fresh: Vec<HiddenRows>) {
        let Some(last) = windows.last() else {
            return;
        };
        let fresh_runs = |window: &Window| &fresh[window.fresh.clone()];
        if windows
            .iter()
            .all(|window| window.runs.len() == window.fresh.len())
        {
            let mut shift = &Shift::default();
            let mut next = 0;
            for window in windows {
                shift.apply(&mut self.hidden[next..window.runs.start]);
                self.hidden[window.runs.clone()].clone_from_slice(fresh_runs(window));
                (shift, next) = (&window.shift_after, window.runs.end);
            }
            last.shift_after.apply(&mut self.hidden[next..]);
            return;
        }
        let added: usize = windows.iter().map(|window| window.fresh.len()).sum();
        let removed: usize = windows.iter().map(|window| window.runs.len()).sum();
        let mut runs = Vec::with_capacity(self.hidden.len() + added - removed);
        let mut shift = &Shift::default();
        let mut next = 0;
        for window in windows {
            runs.extend(
                self.hidden[next..window.runs.start]
                    .iter()
                    .map(|run| shift.shifted(run)),
            );
            runs.extend_from_slice(fresh_runs(window));
            (shift, next) = (&window.shift_after, window.runs.end);
        }
        runs.extend(self.hidden[next..].iter().map(|run| shift.shifted(run)));
        self.hidden = runs;
    }

    /// Installs the projection for `wrap_row_count` wrap rows with
    /// `hidden_ranges` hidden, merging overlapping and adjacent ranges.
    #[cfg(test)]
    fn set_hidden_rows(&mut self, wrap_row_count: usize, mut hidden_ranges: Vec<Range<usize>>) {
        hidden_ranges.sort_by_key(|range| range.start);
        let runs = hidden_ranges
            .into_iter()
            .map(|range| (range.start as u32..range.end as u32, range));
        self.set_sorted_hidden_rows(wrap_row_count, runs);
    }

    /// [`FoldMap::set_hidden_rows`] for runs of buffer rows and their wrap
    /// rows, sorted by start.
    fn set_sorted_hidden_rows(
        &mut self,
        wrap_row_count: usize,
        runs: impl IntoIterator<Item = (Range<u32>, Range<usize>)>,
    ) {
        self.hidden.clear();
        self.total_hidden = 0;
        self.wrap_row_count = wrap_row_count;
        for (buffer_rows, wrap_rows) in runs {
            push_run(
                &mut self.hidden,
                &mut self.total_hidden,
                wrap_row_count,
                buffer_rows,
                wrap_rows,
            );
        }
    }
}

/// Appends `wrap_rows`, which show `buffer_rows`, to `runs` as a hidden run,
/// clipped to `wrap_row_count` and merged with the last run if they overlap
/// or touch. `total_hidden` counts the rows `runs` hide.
fn push_run(
    runs: &mut Vec<HiddenRows>,
    total_hidden: &mut usize,
    wrap_row_count: usize,
    buffer_rows: Range<u32>,
    wrap_rows: Range<usize>,
) {
    let wrap_rows = wrap_rows.start..wrap_rows.end.min(wrap_row_count);
    if wrap_rows.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && wrap_rows.start <= last.wrap_rows.end
    {
        if wrap_rows.end > last.wrap_rows.end {
            *total_hidden += wrap_rows.end - last.wrap_rows.end;
            last.wrap_rows.end = wrap_rows.end;
            last.buffer_rows.end = last.buffer_rows.end.max(buffer_rows.end);
        }
        return;
    }
    *total_hidden += wrap_rows.len();
    runs.push(HiddenRows {
        hidden_before: *total_hidden - wrap_rows.len(),
        wrap_rows,
        buffer_rows,
    });
}

/// The buffer rows `folds`, sorted by start, hide, merged into runs.
///
/// Every buffer row has at least one wrap row, in order, so the buffer rows
/// folds hide merge into runs as their wrap rows would. Merged first, the
/// runs' bounds ascend, and one cursor finds them.
fn buffer_runs(folds: &[Fold]) -> impl Iterator<Item = Range<u32>> + Clone {
    let mut rest = folds.iter().peekable();
    iter::from_fn(move || {
        let fold = rest.next()?;
        let mut end = fold.end;
        while let Some(next) = rest.next_if(|next| next.start <= end) {
            end = end.max(next.end);
        }
        Some(fold.start + 1..end + 1)
    })
}

/// Each of `buffer_runs`, which ascend, with its wrap rows in `wrap_map`.
fn resolve(
    wrap_map: &WrapMap,
    buffer_runs: impl Iterator<Item = Range<u32>> + Clone,
) -> impl Iterator<Item = (Range<u32>, Range<usize>)> {
    let bounds = buffer_runs
        .clone()
        .flat_map(|rows| [rows.start as usize, rows.end as usize]);
    let mut bounds = wrap_map.first_rows(bounds);
    buffer_runs.map(move |rows| {
        let start = bounds.next().expect("a start per run");
        (rows, start..bounds.next().expect("an end per run"))
    })
}

/// One edit, or several merged: buffer rows `old`, before any of the
/// edits, are rows `new` after them all, and have `wrap_delta` more wrap
/// rows.
#[derive(Debug)]
struct ComposedEdit {
    old: Range<u32>,
    new: Range<u32>,
    wrap_delta: isize,
}

/// `edits`, each in the coordinates the ones before it left, as disjoint
/// edits of the rows before them all, sorted. Edits that overlap or touch
/// merge.
///
/// Edits in order, as a batch's are, touch only the last few composed so
/// far, so composing them costs their number.
fn compose(edits: &[WrapEdit]) -> Vec<ComposedEdit> {
    let delta = |c: &ComposedEdit| c.new.len() as i32 - c.old.len() as i32;
    let mut composed: Vec<ComposedEdit> = Vec::new();
    // The rows all of `composed` add.
    let mut total = 0;
    for edit in edits {
        let Splice {
            start, old_rows, ..
        } = edit.buffer;
        let (start, end) = (start as u32, (start + old_rows) as u32);
        let edit_delta = edit.buffer.delta() as i32;
        // Those overlapping or touching `start..end` in the rows so far.
        let first = composed.partition_point(|c| c.new.end < start);
        let last = composed.partition_point(|c| c.new.start <= end);
        let before = total - composed[first..].iter().map(delta).sum::<i32>();
        let mut merged = ComposedEdit {
            old: start.wrapping_add_signed(-before)..end.wrapping_add_signed(-before),
            new: start..end,
            wrap_delta: edit.wrap.delta(),
        };
        let mut inner = before;
        for c in &composed[first..last] {
            if c.new.start < merged.new.start {
                merged.old.start = c.old.start;
                merged.new.start = c.new.start;
            }
            inner += delta(c);
            if c.new.end >= merged.new.end {
                merged.new.end = c.new.end;
                merged.old.end = c.old.end;
            } else {
                // Rows after `c` up to the end only shift.
                merged.old.end = merged.new.end.wrapping_add_signed(-inner);
            }
            merged.wrap_delta += c.wrap_delta;
        }
        merged.new.end = merged.new.end.wrapping_add_signed(edit_delta);
        for c in &mut composed[last..] {
            c.new = c.new.start.wrapping_add_signed(edit_delta)
                ..c.new.end.wrapping_add_signed(edit_delta);
        }
        composed.splice(first..last, [merged]);
        total += edit_delta;
    }
    composed
}

/// Old runs `runs` to replace with `fresh` (indices into a list of new
/// runs), the runs after them to move by `shift_after`.
struct Window {
    runs: Range<usize>,
    fresh: Range<usize>,
    shift_after: Shift,
}

/// How far runs move: by the edits before them.
#[derive(Clone, Copy, Default)]
struct Shift {
    buffer: isize,
    wrap: isize,
    hidden: isize,
}

impl Shift {
    fn add(&mut self, edit: &ComposedEdit) {
        self.buffer += edit.new.len() as isize - edit.old.len() as isize;
        self.wrap += edit.wrap_delta;
    }

    fn is_zero(&self) -> bool {
        self.buffer == 0 && self.wrap == 0 && self.hidden == 0
    }

    fn apply(&self, runs: &mut [HiddenRows]) {
        if self.is_zero() {
            return;
        }
        for run in runs {
            *run = self.shifted(run);
        }
    }

    fn shifted(&self, run: &HiddenRows) -> HiddenRows {
        let buffer = self.buffer as i32;
        HiddenRows {
            buffer_rows: run.buffer_rows.start.wrapping_add_signed(buffer)
                ..run.buffer_rows.end.wrapping_add_signed(buffer),
            wrap_rows: run.wrap_rows.start.wrapping_add_signed(self.wrap)
                ..run.wrap_rows.end.wrapping_add_signed(self.wrap),
            hidden_before: run.hidden_before.wrapping_add_signed(self.hidden),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// The projection as per-row tables: display row to wrap row, and wrap
    /// row to display row.
    fn dense_projection(
        wrap_row_count: usize,
        hidden: &[Range<usize>],
    ) -> (Vec<usize>, Vec<Option<usize>>) {
        let mut visible = Vec::new();
        let mut display_rows = vec![None; wrap_row_count];
        for (wrap_row, display_row) in display_rows.iter_mut().enumerate() {
            if !hidden.iter().any(|range| range.contains(&wrap_row)) {
                *display_row = Some(visible.len());
                visible.push(wrap_row);
            }
        }
        (visible, display_rows)
    }

    proptest! {
        /// Every row maps to a display row and back, or is hidden and maps
        /// to the visible row before it, exactly as the dense tables say.
        #[test]
        fn runs_match_the_dense_projection(
            hidden in prop::collection::vec((0usize..24, 0usize..8), 0..5),
        ) {
            let wrap_row_count = 20;
            let hidden: Vec<Range<usize>> =
                hidden.into_iter().map(|(start, len)| start..start + len).collect();
            let mut fold_map = FoldMap::default();
            fold_map.set_hidden_rows(wrap_row_count, hidden.clone());
            let (visible, display_rows) = dense_projection(wrap_row_count, &hidden);

            prop_assert_eq!(fold_map.display_row_count(), visible.len());
            for wrap_row in 0..wrap_row_count + 2 {
                let display_row = display_rows.get(wrap_row).copied().flatten();
                prop_assert_eq!(fold_map.to_display(wrap_row), display_row);
                if let Some(display_row) = display_row {
                    prop_assert_eq!(fold_map.to_wrap(display_row), Some(wrap_row));
                }
                let nearest = match visible.binary_search(&wrap_row) {
                    Ok(index) => index,
                    Err(index) => index.saturating_sub(1),
                };
                prop_assert_eq!(fold_map.nearest_display_row(wrap_row), nearest);
                prop_assert_eq!(
                    fold_map.hidden_run(wrap_row).is_some(),
                    wrap_row < wrap_row_count && display_row.is_none()
                );
            }
            for display_row in 0..wrap_row_count + 2 {
                prop_assert_eq!(fold_map.to_wrap(display_row), visible.get(display_row).copied());
            }
        }
    }

    #[test]
    fn edits_shift_keep_or_drop_folds() {
        // (edit start, old rows, new rows) => fold 10..=15 afterwards
        let cases = [
            ((2, 1, 1), Some((10, 15))),
            ((2, 1, 3), Some((12, 17))),
            ((2, 3, 1), Some((8, 13))),
            // Typing on the header keeps the fold; splitting it doesn't.
            ((10, 1, 1), Some((10, 15))),
            ((10, 1, 2), None),
            // Joining the header to the row above.
            ((9, 2, 1), None),
            // Anything inside, including the last hidden row.
            ((12, 1, 1), None),
            ((15, 1, 1), None),
            // Right after the fold.
            ((16, 1, 3), Some((10, 15))),
            // Whole rows inserted before the header, after it, or inside.
            ((10, 0, 2), Some((12, 17))),
            ((16, 0, 2), Some((10, 15))),
            ((11, 0, 2), None),
        ];
        for ((start, old_rows, new_rows), want) in cases {
            let mut fold_map = FoldMap::default();
            fold_map.fold(10..=15);
            fold_map.edit(start, old_rows, new_rows);
            let got = fold_map.folds().first().map(|fold| (fold.start, fold.end));
            assert_eq!(got, want, "edit at {start}, {old_rows} => {new_rows} rows");
        }
    }

    /// One sync of a large batch of edits, each dropping a different fold,
    /// costs about the number of runs plus edits: no run moves once per
    /// edit, and no edit rescans those before it. Done per edit, this many
    /// would take minutes in a debug build.
    #[test]
    fn a_sync_of_many_edits_is_one_pass_over_the_runs() {
        use fe_text::Text;

        let rows = 1_000_000;
        let text = Text::new(&"\n".repeat(rows as usize - 1));
        let wrap_map = WrapMap::new(&text, 4);
        let mut fold_map = FoldMap::default();
        fold_map.fold_many((0..rows).step_by(5).map(|row| row..=row + 3));
        fold_map.rebuild(&wrap_map, rows);
        assert_eq!(fold_map.hidden.len(), 200_000);

        // Typing on a hidden row of every fourth fold: no rows move, and
        // each edit drops its fold and with it a run.
        let edits: Vec<WrapEdit> = (0..rows)
            .step_by(20)
            .map(|row| {
                let splice = Splice {
                    start: row as usize + 2,
                    old_rows: 1,
                    new_rows: 1,
                };
                WrapEdit {
                    buffer: splice,
                    wrap: splice,
                }
            })
            .collect();
        // What `FoldMap::edit` leaves of the folds, without its pass over
        // them per edit.
        fold_map.folds.retain(|fold| fold.start % 20 != 0);
        assert_eq!(fold_map.folds().len(), 150_000);

        let started = std::time::Instant::now();
        fold_map.sync(&wrap_map, rows, &edits);
        let elapsed = started.elapsed();
        let synced = fold_map.hidden.clone();
        fold_map.rebuild(&wrap_map, rows);
        assert_eq!(synced, fold_map.hidden);
        assert!(
            elapsed.as_secs_f64() < 2.,
            "50k edits synced in {elapsed:?}"
        );
    }

    #[test]
    fn fold_many_folds_as_fold_does() {
        let rows = [4..=6, 1..=8, 2..=2, 4..=6, 0..=3];
        let mut one_at_a_time = FoldMap::default();
        for rows in rows.clone() {
            one_at_a_time.fold(rows);
        }
        let mut at_once = FoldMap::default();
        at_once.fold(1..=8);
        at_once.fold_many(rows);
        assert_eq!(at_once.folds(), one_at_a_time.folds());
    }
}
