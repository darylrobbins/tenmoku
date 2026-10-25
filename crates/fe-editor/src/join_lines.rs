//! Join Lines, as VS Code's `editor.action.joinLines`.
//!
//! Each selection joins the rows it touches (as [`touched_rows`] reads
//! them) into one; an empty selection, or one inside a row, joins its row
//! with the next. At each join the next row's leading whitespace goes and a
//! single space takes its place, unless the row joined onto is empty, the
//! rest of the next row is empty, or it starts with `)`. The last row has
//! nothing to join. The command makes its edits as one transaction.

use std::{collections::BTreeSet, ops::Range};

use fe_buffer::ResolvedSelection;
use fe_command::{Effect, Registry};

use crate::{
    EditorModel, caret,
    commands::{Editor, add},
    rows::touched_rows,
};

/// Registers the join command. It edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "join_lines", "Join Lines", true, |model, _| {
        model.join_lines();
        Ok(Effect::Changed)
    });
}

/// Where `offset` is once `edits`, sorted and disjoint, are applied: at the
/// start of a deletion it's in, and before an insertion at it.
fn map_before(edits: &[(Range<usize>, String)], offset: usize) -> usize {
    let mut delta = 0isize;
    for (range, new_text) in edits {
        if range.start > offset || range.start == offset && range.is_empty() {
            break;
        }
        if offset < range.end {
            return range.start.saturating_add_signed(delta);
        }
        delta += new_text.len() as isize - range.len() as isize;
    }
    offset.saturating_add_signed(delta)
}

impl EditorModel {
    /// Joins each selection's rows. An empty selection ends up a caret at
    /// the (first) join point; any other selection stays over its text.
    pub fn join_lines(&mut self) {
        self.sync_display();
        let text = self.text();
        let last_row = text.row_count() - 1;
        let all = self.selections();
        // Row `r` joins the row after it, for each `r` here.
        let mut joins = BTreeSet::new();
        for selection in &all {
            let touched = touched_rows(text, &selection.range);
            let (first, last) = (*touched.start(), *touched.end());
            if first == last {
                // A selection that ends at the start of the next row leaves
                // that row alone; only a caret, or one inside a row, joins.
                let within = text.offset_to_point(selection.range.end).row == first;
                if (selection.range.is_empty() || within) && first < last_row {
                    joins.insert(first);
                }
            } else {
                joins.extend(first..last);
            }
        }
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        // Each join's edit, by the row it joins onto.
        let mut points: Vec<(u32, usize)> = Vec::new();
        // Whether the row joined onto, with the rows already joined to it,
        // is empty.
        let mut empty = false;
        for &row in &joins {
            if !joins.contains(&(row.wrapping_sub(1))) || row == 0 {
                empty = text.row_len(row) == 0;
            }
            let end = text.row_start(row) + text.row_len(row) as usize;
            let next = text.row_to_string(row + 1);
            let trimmed = next.trim_start();
            let leading = next.len() - trimmed.len();
            let space = !empty && !trimmed.is_empty() && !trimmed.starts_with(')');
            empty = empty && trimmed.is_empty();
            let next_start = text.row_start(row + 1);
            points.push((row, edits.len()));
            edits.push((
                end..next_start + leading,
                if space { " " } else { "" }.to_owned(),
            ));
        }
        let selections = all
            .iter()
            .map(|selection| {
                let range = &selection.range;
                if !range.is_empty() {
                    return ResolvedSelection {
                        range: map_before(&edits, range.start)..map_before(&edits, range.end),
                        reversed: selection.reversed,
                    };
                }
                let row = text.offset_to_point(range.start).row;
                match points.iter().find(|&&(joined, _)| joined == row) {
                    Some(&(_, index)) => {
                        let (at, _) = &edits[index];
                        caret(map_before(&edits, at.start))
                    }
                    None => caret(map_before(&edits, range.start)),
                }
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
    }
}

#[cfg(test)]
#[path = "join_lines_tests.rs"]
mod tests;
