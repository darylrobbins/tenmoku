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
//! of folds and mapping either way is a binary search.

use std::ops::{Range, RangeInclusive};

use crate::wrap_map::WrapMap;

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
        let hidden = self.folds.iter().map(|fold| {
            let start = wrap_map.first_row(fold.start as usize + 1);
            let end = wrap_map.first_row(fold.end as usize + 1);
            start..end
        });
        self.set_hidden_rows(wrap_map.row_count(), hidden.collect());
    }

    /// Installs the projection for `wrap_row_count` wrap rows with
    /// `hidden_ranges` hidden, merging overlapping and adjacent ranges.
    fn set_hidden_rows(&mut self, wrap_row_count: usize, mut hidden_ranges: Vec<Range<usize>>) {
        hidden_ranges.sort_by_key(|range| range.start);

        self.hidden.clear();
        self.total_hidden = 0;
        self.wrap_row_count = wrap_row_count;
        for range in hidden_ranges {
            let range = range.start..range.end.min(wrap_row_count);
            if range.is_empty() {
                continue;
            }
            if let Some(last) = self.hidden.last_mut()
                && range.start <= last.wrap_rows.end
            {
                if range.end > last.wrap_rows.end {
                    self.total_hidden += range.end - last.wrap_rows.end;
                    last.wrap_rows.end = range.end;
                }
                continue;
            }
            self.total_hidden += range.len();
            self.hidden.push(HiddenRows {
                hidden_before: self.total_hidden - range.len(),
                wrap_rows: range,
            });
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
