//! Folding ranges a host gives an editor ([`EditorView::set_fold_ranges`]):
//! a language server's `textDocument/foldingRange`. While they're set, fold,
//! unfold, fold all, the gutter's chevrons and a click on one use them alone
//! in place of the syntax tree's candidates, as VS Code's provider-first
//! `editor.foldingStrategy: "auto"` does, and a language without a grammar
//! folds. They're anchored in the buffer and follow edits until the host's
//! next answer replaces them.

use std::cell::RefCell;
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use fe_buffer::{BufferSnapshot, Version};

use crate::EditorView;

/// The ranges a view folds with, as rows of the buffer at `synced`.
pub(crate) struct ServerFolds {
    state: RefCell<State>,
}

struct State {
    synced: Version,
    /// Each range's header and last hidden row, as the offsets their rows
    /// start at, which edits move.
    offsets: Vec<(usize, usize)>,
    /// The ranges at `synced`: sorted, sanitised.
    rows: Rc<Vec<RangeInclusive<u32>>>,
}

/// `ranges` as VS Code's syntax range provider keeps them: sorted by start,
/// one per start row (the widest), none under two rows, and none that
/// starts inside another and ends after it. A range nested in another may
/// share its last row.
pub(crate) fn sanitize(mut ranges: Vec<RangeInclusive<u32>>) -> Vec<RangeInclusive<u32>> {
    ranges.retain(|range| range.end() > range.start());
    ranges.sort_by_key(|range| (*range.start(), std::cmp::Reverse(*range.end())));
    ranges.dedup_by_key(|range| *range.start());
    // The ranges still open at the one being looked at, outermost first.
    let mut open: Vec<RangeInclusive<u32>> = Vec::new();
    let mut kept = Vec::with_capacity(ranges.len());
    for range in ranges {
        while open.last().is_some_and(|top| top.end() < range.start()) {
            open.pop();
        }
        if open.last().is_some_and(|top| range.end() > top.end()) {
            continue;
        }
        open.push(range.clone());
        kept.push(range);
    }
    kept
}

impl ServerFolds {
    /// Folds for `ranges` (rows of `snapshot`, clipped to its last row).
    fn new(snapshot: &BufferSnapshot, ranges: Vec<RangeInclusive<u32>>) -> Self {
        let text = snapshot.text();
        let last = text.row_count() - 1;
        let ranges = ranges
            .into_iter()
            .map(|range| *range.start()..=(*range.end()).min(last))
            .collect();
        let rows = sanitize(ranges);
        let offsets = (rows.iter())
            .map(|range| (text.row_start(*range.start()), text.row_start(*range.end())))
            .collect();
        Self {
            state: RefCell::new(State {
                synced: snapshot.version(),
                offsets,
                rows: Rc::new(rows),
            }),
        }
    }

    /// The ranges as rows of `snapshot`, moved on from the version they
    /// were last asked at: a range with an edit's replaced or deleted text
    /// strictly around the start of its header or last row, or that
    /// collapsed to one row, is dropped.
    pub(crate) fn current(&self, snapshot: &BufferSnapshot) -> Rc<Vec<RangeInclusive<u32>>> {
        let mut state = self.state.borrow_mut();
        if state.synced == snapshot.version() {
            return state.rows.clone();
        }
        let flat: Vec<usize> = (state.offsets.iter())
            .flat_map(|&(start, end)| [start, end])
            .collect();
        let moved = crate::inlay_hints::follow(snapshot, state.synced, &flat);
        let text = snapshot.text();
        let mut rows = Vec::with_capacity(state.offsets.len());
        for pair in moved.chunks(2) {
            let (Some(start), Some(end)) = (pair[0], pair[1]) else {
                continue;
            };
            let (start, end) = (
                text.offset_to_point(start).row,
                text.offset_to_point(end).row,
            );
            if end > start {
                rows.push(start..=end);
            }
        }
        // Sanitised again: an edit may have made ranges overlap.
        let rows = sanitize(rows);
        let offsets = (rows.iter())
            .map(|range| (text.row_start(*range.start()), text.row_start(*range.end())))
            .collect();
        state.synced = snapshot.version();
        state.offsets = offsets;
        state.rows = Rc::new(rows);
        state.rows.clone()
    }
}

/// The innermost of `ranges` headed by `row`, or if there's none, the
/// innermost one containing it.
pub(crate) fn range_at(ranges: &[RangeInclusive<u32>], row: u32) -> Option<RangeInclusive<u32>> {
    let headed = ranges.iter().find(|range| *range.start() == row);
    let containing = (ranges.iter())
        .filter(|range| range.contains(&row))
        .max_by_key(|range| *range.start());
    headed.or(containing).cloned()
}

/// The ranges of `ranges` whose header is in `rows`.
pub(crate) fn ranges_in(
    ranges: &[RangeInclusive<u32>],
    rows: Range<u32>,
) -> Vec<RangeInclusive<u32>> {
    (ranges.iter())
        .filter(|range| rows.contains(range.start()))
        .cloned()
        .collect()
}

impl EditorView {
    /// Folds with `ranges` in place of the syntax tree's, or with the
    /// tree's again if `None`. Each is a header row to the last row it
    /// hides, in rows of the buffer as it is now (the shape of
    /// `fe_syntax::fold_ranges`: an LSP range's `startLine..=endLine`). They
    /// are sanitised as VS Code does, follow edits until the next call, and
    /// leave the folds already made alone.
    pub fn set_fold_ranges(&mut self, ranges: Option<Vec<RangeInclusive<u32>>>) {
        self.model.sync_display();
        self.server_folds = ranges.map(|ranges| ServerFolds::new(self.model.snapshot(), ranges));
    }

    /// The host's fold ranges now, if it has given any.
    pub(crate) fn server_fold_ranges(&self) -> Option<Rc<Vec<RangeInclusive<u32>>>> {
        let folds = self.server_folds.as_ref()?;
        Some(folds.current(self.model.snapshot()))
    }
}

#[cfg(test)]
#[path = "server_folds_tests.rs"]
mod tests;
