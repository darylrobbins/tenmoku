//! Fold Level 1 to 7, Fold Recursively and Unfold Recursively, as VS Code's
//! `editor.foldLevelN`, `editor.foldRecursively` and
//! `editor.unfoldRecursively`.
//!
//! The candidates are the host's, or with none the indented blocks, as Fold
//! All finds them. A candidate's level is one more than the number of
//! candidates around it, the outermost of those sharing a header row
//! standing for them all.

use std::ops::RangeInclusive;

use fe_buffer::Anchor;
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, EditorHost, add},
    indented_block, indented_blocks,
};

/// The level commands: name, title and the level each folds.
const LEVELS: [(&str, &str, u32); 7] = [
    ("fold_level_1", "Fold Level 1", 1),
    ("fold_level_2", "Fold Level 2", 2),
    ("fold_level_3", "Fold Level 3", 3),
    ("fold_level_4", "Fold Level 4", 4),
    ("fold_level_5", "Fold Level 5", 5),
    ("fold_level_6", "Fold Level 6", 6),
    ("fold_level_7", "Fold Level 7", 7),
];

/// Registers the level commands and the two recursive ones. None edits.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    for (name, title, level) in LEVELS {
        add(registry, name, title, false, move |model, host| {
            let candidates = all_candidates(model, host);
            model.fold_level(level, candidates);
            Ok(Effect::Changed)
        });
    }
    add(
        registry,
        "fold_recursively",
        "Fold Recursively",
        false,
        |model, host| {
            let candidates = all_candidates(model, host);
            model.fold_recursively(&|row| host.fold_range_at(row), candidates);
            Ok(Effect::Changed)
        },
    );
    add(
        registry,
        "unfold_recursively",
        "Unfold Recursively",
        false,
        |model, host| {
            model.unfold_recursively(&|row| host.fold_range_at(row));
            Ok(Effect::Changed)
        },
    );
}

fn all_candidates(model: &EditorModel, host: &dyn EditorHost) -> Vec<RangeInclusive<u32>> {
    host.fold_ranges(0..model.text().row_count())
}

impl EditorModel {
    /// Folds the candidates at `level` (1 is the outermost) but those
    /// containing a selection's first or last row, keeping the folds there
    /// are. `candidates` are rows of [`EditorModel::text`]; with none, the
    /// indented blocks of the buffer as it is now are.
    pub fn fold_level(
        &mut self,
        level: u32,
        candidates: impl IntoIterator<Item = RangeInclusive<u32>>,
    ) {
        let candidates = self.candidates_or_blocks(candidates);
        let rows = self.selection_rows();
        let folds: Vec<_> = levels(candidates)
            .into_iter()
            .filter(|(at, range)| *at == level && !rows.iter().any(|row| range.contains(row)))
            .map(|(_, range)| range)
            .collect();
        // Fold All folds every indented block when given nothing.
        if !folds.is_empty() {
            self.fold_all(folds);
        }
    }

    /// Folds, for each caret, the candidate at its row (as
    /// [`EditorModel::fold`] finds it) and every one of `candidates` inside
    /// it. Carets folded away move to the fold's header.
    ///
    /// `candidate_at` and `candidates` give rows of [`EditorModel::text`]:
    /// those folds are made there and then caught up with the buffer. A
    /// caret with no candidate folds the indented block it heads or is in,
    /// found in the buffer as it is once caught up, and the blocks and
    /// `candidates` (followed through the edits) inside that.
    pub fn fold_recursively(
        &mut self,
        candidate_at: &dyn Fn(u32) -> Option<RangeInclusive<u32>>,
        candidates: impl IntoIterator<Item = RangeInclusive<u32>>,
    ) {
        let candidates: Vec<_> = candidates.into_iter().collect();
        let mut folds = Vec::new();
        let mut without = Vec::new();
        for head in self.head_anchors() {
            match candidate_at(self.row_of(head)) {
                Some(outer) => folds.extend(with_inside(outer, &candidates)),
                None => without.push(head),
            }
        }
        if !folds.is_empty() {
            self.display.fold_many(folds);
        }
        // Followed through the edits catching up takes in, to nest in an
        // indented block too.
        let anchored = if without.is_empty() {
            Vec::new()
        } else {
            self.anchor_rows(&candidates)
        };
        self.sync_display();
        if !without.is_empty() {
            let mut inside = indented_blocks(self.text(), self.tab_size);
            inside.extend(anchored.into_iter().filter_map(|(start, end)| {
                let (start, end) = (self.row_of(start), self.row_of(end));
                (start < end).then_some(start..=end)
            }));
            let folds: Vec<_> = (without.into_iter())
                .filter_map(|head| indented_block(self.text(), self.row_of(head), self.tab_size))
                .flat_map(|outer| with_inside(outer, &inside))
                .collect();
            self.display.fold_many(folds);
        }
        self.move_folded_carets();
    }

    /// Unfolds, for each caret, the folds headed on its row and those inside
    /// the candidate at it (as [`EditorModel::fold`] finds it), so the
    /// candidate and everything in it open; folds beside it stay, even one
    /// sharing its last row.
    ///
    /// `candidate_at` gives rows of [`EditorModel::text`], and those folds
    /// open there before catching up with the buffer. A caret with no
    /// candidate opens the indented block it heads or is in, found in the
    /// buffer as it is once caught up.
    pub fn unfold_recursively(
        &mut self,
        candidate_at: &dyn Fn(u32) -> Option<RangeInclusive<u32>>,
    ) {
        let mut targets = Vec::new();
        let mut without = Vec::new();
        for head in self.head_anchors() {
            let row = self.row_of(head);
            match candidate_at(row) {
                Some(rows) => targets.push((row, rows)),
                None => without.push(head),
            }
        }
        self.unfold_inside(&targets);
        self.sync_display();
        let targets: Vec<_> = (without.into_iter())
            .map(|head| {
                let row = self.row_of(head);
                let block = indented_block(self.text(), row, self.tab_size);
                (row, block.unwrap_or(row..=row))
            })
            .collect();
        self.unfold_inside(&targets);
    }

    /// Unfolds the folds headed on each target's row or inside its rows,
    /// keeping the rest.
    fn unfold_inside(&mut self, targets: &[(u32, RangeInclusive<u32>)]) {
        let opens = |start: u32, end: u32| {
            (targets.iter())
                .any(|(row, rows)| start == *row || (rows.start() <= &start && &end <= rows.end()))
        };
        let folds = self.display.folds();
        let kept: Vec<_> = (folds.iter())
            .filter(|fold| !opens(fold.start, fold.end))
            .map(|fold| fold.start..=fold.end)
            .collect();
        if kept.len() != folds.len() {
            self.display.unfold_all();
            self.display.fold_many(kept);
        }
    }

    /// `candidates`, or with none, the indented blocks of the buffer once
    /// caught up with.
    fn candidates_or_blocks(
        &mut self,
        candidates: impl IntoIterator<Item = RangeInclusive<u32>>,
    ) -> Vec<RangeInclusive<u32>> {
        let candidates: Vec<_> = candidates.into_iter().collect();
        if !candidates.is_empty() {
            return candidates;
        }
        self.sync_display();
        indented_blocks(self.text(), self.tab_size)
    }

    /// Where each selection's head is, to resolve before and after catching
    /// up.
    fn head_anchors(&self) -> Vec<Anchor> {
        (self.selections.iter())
            .map(|selection| match selection.reversed {
                true => selection.start,
                false => selection.end,
            })
            .collect()
    }

    /// Anchors for each of `rows`, rows of [`EditorModel::text`]: after its
    /// first row's start and before its last row's end, so rows inserted
    /// before or after it leave it out.
    fn anchor_rows(&self, rows: &[RangeInclusive<u32>]) -> Vec<(Anchor, Anchor)> {
        let text = self.text();
        (rows.iter())
            .filter(|rows| *rows.end() < text.row_count())
            .map(|rows| {
                let end = text.row_start(*rows.end()) + text.row_len(*rows.end()) as usize;
                (
                    self.snapshot.anchor_after(text.row_start(*rows.start())),
                    self.snapshot.anchor_before(end),
                )
            })
            .collect()
    }

    /// The row of [`EditorModel::text`] `anchor` is on.
    fn row_of(&self, anchor: Anchor) -> u32 {
        let offset = self.snapshot.resolve(&anchor);
        self.text().offset_to_point(offset).row
    }

    /// The first and last row of each selection, in [`EditorModel::text`].
    fn selection_rows(&self) -> Vec<u32> {
        let text = self.text();
        (self.selections().iter())
            .flat_map(|selection| {
                let range = &selection.range;
                [range.start, range.end].map(|offset| text.offset_to_point(offset).row)
            })
            .collect()
    }
}

/// `outer` and the ones of `candidates` inside it.
fn with_inside(
    outer: RangeInclusive<u32>,
    candidates: &[RangeInclusive<u32>],
) -> impl Iterator<Item = RangeInclusive<u32>> + '_ {
    let (start, end) = (*outer.start(), *outer.end());
    let inside = (candidates.iter())
        .filter(move |range| start <= *range.start() && *range.end() <= end)
        .cloned();
    inside.chain([outer])
}

/// Each candidate with its level, sorted, those sharing a header row
/// dropped for the outermost of them. A candidate overlapping another
/// without nesting in it is beside it.
fn levels(mut candidates: Vec<RangeInclusive<u32>>) -> Vec<(u32, RangeInclusive<u32>)> {
    candidates.sort_by_key(|range| (*range.start(), std::cmp::Reverse(*range.end())));
    candidates.dedup_by_key(|range| *range.start());
    // The ends of the candidates around the one being placed, outermost
    // first.
    let mut open: Vec<u32> = Vec::new();
    candidates
        .into_iter()
        .map(|range| {
            while open.last().is_some_and(|&end| end < *range.end()) {
                open.pop();
            }
            open.push(*range.end());
            (open.len() as u32, range)
        })
        .collect()
}

#[cfg(test)]
#[path = "fold_levels_tests.rs"]
mod tests;
