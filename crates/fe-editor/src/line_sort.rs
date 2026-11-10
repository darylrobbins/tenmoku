//! Sort Lines Ascending/Descending and Delete Duplicate Lines, as VS Code's
//! `editor.action.sortLinesAscending`, `sortLinesDescending` and
//! `removeDuplicateLines`.
//!
//! With one empty selection the range is the whole document (not counting
//! the empty row after a final line break); otherwise it is the rows each
//! selection touches, as [`EditorModel::blocks`] reads them. Each command
//! makes its edits as one transaction.
//!
//! Rows compare with ICU4X's collator for the root locale (`und`) at its
//! default tertiary strength, the collation `Intl.Collator` and so VS Code's
//! `localeCompare` use: `a` sorts before `B`, `é` after `e`, digits before
//! letters, and `z` before `þ`. Rows the collator calls equal compare by code
//! point, so the order is total. The sort is stable.

use std::{cmp::Ordering, ops::Range};

use fe_buffer::ResolvedSelection;
use fe_command::{Effect, Registry};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences, options::CollatorOptions};

use crate::{
    EditorModel,
    commands::{Editor, add},
    rows::{map_offset, touched_rows},
};

/// Registers the line commands. Each edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "sort_lines_ascending", true, |model, _| {
        model.sort_lines(false);
        Ok(Effect::Changed)
    });
    add(registry, "sort_lines_descending", true, |model, _| {
        model.sort_lines(true);
        Ok(Effect::Changed)
    });
    add(registry, "delete_duplicate_lines", true, |model, _| {
        model.delete_duplicate_lines();
        Ok(Effect::Changed)
    });
}

/// Compares two rows' text by root collation, then by code point.
fn compare(a: &str, b: &str) -> Ordering {
    // The collator isn't `Sync`, so each thread builds its own.
    thread_local! {
        static COLLATOR: CollatorBorrowed<'static> =
            Collator::try_new(CollatorPreferences::default(), CollatorOptions::default())
                .expect("compiled collation data for the root locale");
    }
    COLLATOR
        .with(|collator| collator.compare(a, b))
        .then_with(|| a.cmp(b))
}

/// Whether `offset` is in one of the deleted rows' spans.
fn deleted_at(deleted: &[Range<usize>], offset: usize) -> bool {
    deleted
        .iter()
        .any(|span| span.start <= offset && offset <= span.end)
}

/// The rows of one range to work on, and the selections (by index) that
/// touch them.
struct Group {
    rows: Range<u32>,
    selections: Range<usize>,
}

/// Whether the selection's `range` is `[start, end]` or reaches the line
/// break after `end` as well.
fn covers(range: &Range<usize>, start: usize, end: usize) -> bool {
    range.start == start && range.end >= end
}

impl EditorModel {
    /// The ranges to work on: the whole document for one empty selection,
    /// else each selection's touched rows. Selections sharing a row make one
    /// range; adjacent ranges stay apart.
    fn line_groups(&self) -> Vec<Group> {
        let text = self.text();
        let selections = self.selections();
        if selections.len() == 1 && selections[0].range.is_empty() {
            let mut end = text.row_count();
            if end > 1 && text.row_len(end - 1) == 0 {
                end -= 1;
            }
            return vec![Group {
                rows: 0..end,
                selections: 0..1,
            }];
        }
        let mut groups: Vec<Group> = Vec::new();
        for (index, selection) in selections.iter().enumerate() {
            let touched = touched_rows(text, &selection.range);
            let rows = *touched.start()..*touched.end() + 1;
            match groups.last_mut() {
                Some(group) if rows.start < group.rows.end => {
                    group.rows.end = group.rows.end.max(rows.end);
                    group.selections.end = index + 1;
                }
                _ => groups.push(Group {
                    rows,
                    selections: index..index + 1,
                }),
            }
        }
        groups
    }

    /// Sorts the rows of each range, stably, descending if `descending`. A
    /// range of one row changes nothing. A selection inside a row follows
    /// the row; one covering the whole range stays; any other selection in
    /// a range goes to the range's start.
    pub fn sort_lines(&mut self, descending: bool) {
        self.sync_display();
        let text = self.text();
        let all = self.selections();
        let mut edits = Vec::new();
        let mut selections = all.clone();
        for group in self.line_groups() {
            if group.rows.len() < 2 {
                continue;
            }
            let rows_of: Vec<(Range<usize>, String)> = group
                .rows
                .clone()
                .map(|row| {
                    let start = text.row_start(row);
                    let end = start + text.row_len(row) as usize;
                    (start..end, text.slice_to_string(start..end))
                })
                .collect();
            let mut order: Vec<usize> = (0..rows_of.len()).collect();
            order.sort_by(|&a, &b| {
                let ordering = compare(&rows_of[a].1, &rows_of[b].1);
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
            let range = rows_of[0].0.start..rows_of[rows_of.len() - 1].0.end;
            let sorted: Vec<&str> = order.iter().map(|&i| rows_of[i].1.as_str()).collect();
            let sorted = sorted.join("\n");
            if sorted == text.slice_to_string(range.clone()) {
                continue;
            }
            // Row `order[k]` now starts at the sum of the rows before it.
            let mut new_start = range.start;
            let mut placed = vec![0; rows_of.len()];
            for &i in &order {
                placed[i] = new_start;
                new_start += rows_of[i].1.len() + 1;
            }
            for index in group.selections.clone() {
                let selection = &all[index];
                let (start, end) = (selection.range.start, selection.range.end);
                // Past the range (a caret on an empty last row) or all of it.
                if start > range.end || covers(&selection.range, range.start, range.end) {
                    continue;
                }
                let row_of = |offset: usize| {
                    rows_of
                        .iter()
                        .position(|(old, _)| old.start <= offset && offset <= old.end)
                };
                // Text stays together if the rows it spans keep their order
                // and stay next to each other.
                let together = |first: usize, last: usize| {
                    (first..last)
                        .all(|row| placed[row + 1] == placed[row] + rows_of[row].1.len() + 1)
                };
                // An end at a row's start (or just past the range) is
                // exclusive: the selection takes the row before, with its
                // terminator, and says nothing of the row it ends at.
                let exclusive = (end > start)
                    .then(|| {
                        if end == range.end + 1 {
                            Some(rows_of.len() - 1)
                        } else {
                            row_of(end)
                                .filter(|&row| row > 0 && rows_of[row].0.start == end)
                                .map(|row| row - 1)
                        }
                    })
                    .flatten();
                let last = exclusive.or_else(|| row_of(end));
                selections[index] = match (row_of(start), last) {
                    (Some(a), Some(b)) if a <= b && together(a, b) => ResolvedSelection {
                        range: placed[a] + (start - rows_of[a].0.start)..match exclusive {
                            Some(_) => placed[b] + rows_of[b].1.len() + 1,
                            None => placed[b] + (end - rows_of[b].0.start),
                        },
                        reversed: selection.reversed,
                    },
                    _ => ResolvedSelection {
                        range: range.start..range.start,
                        reversed: false,
                    },
                };
            }
            edits.push((range, sorted));
        }
        // Sorting keeps each range's length, so offsets outside stay put.
        self.apply_row_edits(edits, selections, Vec::new());
    }

    /// Deletes every row of each range whose text an earlier row of it has,
    /// with its line break. A caret in a deleted row, or a selection of only
    /// deleted text, goes to the start of its range; any other selection
    /// keeps the text of it that survives.
    pub fn delete_duplicate_lines(&mut self) {
        self.sync_display();
        let text = self.text();
        let last_row = text.row_count() - 1;
        let all = self.selections();
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        // The rows deleted, as spans of their text, and where each selection
        // collapses to if an end of it is in one.
        let mut deleted: Vec<Range<usize>> = Vec::new();
        let mut collapse: Vec<Option<usize>> = vec![None; all.len()];
        for group in self.line_groups() {
            let mut seen = std::collections::HashSet::new();
            let start_of_group = text.row_start(group.rows.start);
            for index in group.selections.clone() {
                collapse[index] = Some(start_of_group);
            }
            for row in group.rows {
                let start = text.row_start(row);
                let end = start + text.row_len(row) as usize;
                if seen.insert(text.slice_to_string(start..end)) {
                    continue;
                }
                deleted.push(start..end);
                // The break after the row, or for the last row the one before.
                if row < last_row {
                    edits.push((start..text.row_start(row + 1), String::new()));
                    continue;
                }
                // Deleted rows before it end where it starts: one deletion.
                let mut from = start;
                while let Some((before, _)) = edits.last() {
                    if before.end != from {
                        break;
                    }
                    from = before.start;
                    edits.pop();
                }
                edits.push((from - 1..text.len(), String::new()));
            }
        }
        let selections = all
            .into_iter()
            .zip(collapse)
            .map(|(selection, collapse)| {
                let start = map_offset(&edits, selection.range.start);
                let end = map_offset(&edits, selection.range.end);
                // Nothing of the selected text is left: a caret in a deleted
                // row, or a selection of only deleted text.
                let gone = start == end
                    && (selection.range.is_empty() && deleted_at(&deleted, selection.range.start)
                        || !selection.range.is_empty());
                match collapse {
                    Some(at) if gone => {
                        // The range's start, where earlier deletions put it.
                        let at = map_offset(&edits, at);
                        ResolvedSelection {
                            range: at..at,
                            reversed: false,
                        }
                    }
                    _ => ResolvedSelection {
                        range: start..end,
                        reversed: selection.reversed,
                    },
                }
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
    }
}

#[cfg(test)]
#[path = "line_sort_tests.rs"]
mod tests;
