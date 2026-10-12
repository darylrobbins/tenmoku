//! Row commands: indenting and outdenting the rows selections touch, and
//! moving, duplicating and deleting them.
//!
//! The rows a selection touches run from its start's to its end's, except
//! that a non-empty selection ending at the start of a later row leaves
//! that row out. Selections touching the same or adjacent rows form one
//! block. Each command makes its edits as one transaction and keeps the
//! selections on the same text.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Bias, ResolvedSelection, SelectionSet, Text};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel, caret,
    commands::{Editor, add},
};

/// A command's name and title, and the model method it runs.
type Command = (&'static str, &'static str, fn(&mut EditorModel));

/// Registers the row commands. Each edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    let commands: [Command; 6] = [
        ("indent", "Indent", EditorModel::indent_rows),
        ("outdent", "Outdent", EditorModel::outdent_rows),
        ("move_rows_up", "Move Rows Up", |model| {
            model.move_rows(true)
        }),
        ("move_rows_down", "Move Rows Down", |model| {
            model.move_rows(false)
        }),
        (
            "duplicate_rows",
            "Duplicate Rows",
            EditorModel::duplicate_rows,
        ),
        ("delete_rows", "Delete Rows", EditorModel::delete_rows),
    ];
    for (name, title, run) in commands {
        // `Changed` stands only if the text (and with it the selections)
        // did change.
        add(registry, name, title, true, move |model, _| {
            run(model);
            Ok(Effect::Changed)
        });
    }
}

/// Selections touching the same or adjacent rows: those rows, and the
/// selections' indices.
struct Block {
    rows: RangeInclusive<u32>,
    selections: Range<usize>,
}

impl Block {
    fn first(&self) -> u32 {
        *self.rows.start()
    }

    fn last(&self) -> u32 {
        *self.rows.end()
    }
}

impl EditorModel {
    /// Inserts `tab_size` spaces at the start of each touched row that
    /// isn't blank. Offsets at a row's start move right with it.
    pub fn indent_rows(&mut self) {
        self.sync_display();
        let text = self.text();
        let indent = " ".repeat(self.tab_size);
        let edits = self
            .blocks(false)
            .iter()
            .flat_map(|block| block.rows.clone())
            .filter(|&row| !text.row_to_string(row).trim().is_empty())
            .map(|row| {
                let start = text.row_start(row);
                (start..start, indent.clone())
            })
            .collect();
        self.edit_rows_in_place(edits);
    }

    /// Removes a leading tab, or else up to `tab_size` leading spaces, from
    /// each touched row.
    pub fn outdent_rows(&mut self) {
        self.sync_display();
        let text = self.text();
        let edits = self
            .blocks(false)
            .iter()
            .flat_map(|block| block.rows.clone())
            .filter_map(|row| {
                let row_text = text.row_to_string(row);
                let len = if row_text.starts_with('\t') {
                    1
                } else {
                    row_text
                        .bytes()
                        .take(self.tab_size)
                        .take_while(|&byte| byte == b' ')
                        .count()
                };
                let start = text.row_start(row);
                (len > 0).then(|| (start..start + len, String::new()))
            })
            .collect();
        self.edit_rows_in_place(edits);
    }

    /// Swaps each block with the row above (`up`) or below it; a folded
    /// range there counts as one row. Nothing moves if the topmost block
    /// is at the first row (the bottommost at the last).
    pub fn move_rows(&mut self, up: bool) {
        self.sync_display();
        let blocks = self.blocks(true);
        let text = self.text();
        let last_row = text.row_count() - 1;
        let at_edge = if up {
            blocks.first().is_some_and(|block| block.first() == 0)
        } else {
            blocks.last().is_some_and(|block| block.last() == last_row)
        };
        if blocks.is_empty() || at_edge {
            return;
        }
        let selections = self.selections();
        let mut edits = Vec::new();
        let mut moved = Vec::with_capacity(selections.len());
        let mut folds = Vec::new();
        for block in &blocks {
            // The unit swapped with: the row past the block, or the fold
            // it's in.
            let (first, last) = (block.first(), block.last());
            let other = if up {
                let row = first - 1;
                let start = self
                    .display
                    .hidden_rows(row)
                    .map_or(row, |hidden| hidden.start - 1);
                start..=row
            } else {
                let row = last + 1;
                row..=self.fold_end(row)
            };
            let (top, bottom) = if up {
                (*other.start(), last)
            } else {
                (first, *other.end())
            };
            let block_rows = last + 1 - first;
            let other_rows = other.end() + 1 - other.start();
            // The unit's text goes to the block's other side, so the
            // block's own text, and anything anchored in it, stays put.
            let other_text = rows_text(text, other.clone());
            let block_start = text.row_start(first);
            let other_start = text.row_start(*other.start());
            let shift: isize = if up {
                edits.push((other_start..block_start, String::new()));
                if last < last_row {
                    let end = text.row_start(last + 1);
                    edits.push((end..end, format!("{other_text}\n")));
                } else {
                    edits.push((text.len()..text.len(), format!("\n{other_text}")));
                }
                -((block_start - other_start) as isize)
            } else {
                edits.push((block_start..block_start, format!("{other_text}\n")));
                if *other.end() < last_row {
                    let end = text.row_start(other.end() + 1);
                    edits.push((other_start..end, String::new()));
                } else {
                    // The last row: the break before it goes instead.
                    edits.push((other_start - 1..text.len(), String::new()));
                }
                other_text.len() as isize + 1
            };
            // The edits replace as much as they add, so the text's length
            // doesn't change; the end of a selection ending at the start
            // of the row past the block can fall past it when that row was
            // the last.
            let len = text.len();
            for selection in &selections[block.selections.clone()] {
                let at = |offset: usize| offset.saturating_add_signed(shift).min(len);
                moved.push(ResolvedSelection {
                    range: at(selection.range.start)..at(selection.range.end),
                    reversed: selection.reversed,
                });
            }
            let (block_shift, other_shift) = if up {
                (-(other_rows as i64), block_rows as i64)
            } else {
                (other_rows as i64, -(block_rows as i64))
            };
            for fold in self.display.folds() {
                if fold.start < top || fold.end > bottom {
                    continue;
                }
                let in_block = block.rows.contains(&fold.start);
                let by = if in_block { block_shift } else { other_shift };
                let row = |row: u32| (row as i64 + by) as u32;
                folds.push(row(fold.start)..=row(fold.end));
            }
        }
        self.apply_row_edits(edits, moved, folds);
    }

    /// Inserts a copy of each block's rows below it, moving its selections
    /// onto the copy. A folded range counts as one row.
    pub fn duplicate_rows(&mut self) {
        self.sync_display();
        let blocks = self.blocks(true);
        let text = self.text();
        let last_row = text.row_count() - 1;
        let selections = self.selections();
        let mut edits = Vec::new();
        let mut moved = Vec::with_capacity(selections.len());
        let mut delta = 0;
        for block in &blocks {
            let copy = rows_text(text, block.rows.clone());
            if block.last() < last_row {
                let end = text.row_start(block.last() + 1);
                edits.push((end..end, format!("{copy}\n")));
            } else {
                edits.push((text.len()..text.len(), format!("\n{copy}")));
            }
            delta += copy.len() + 1;
            for selection in &selections[block.selections.clone()] {
                moved.push(ResolvedSelection {
                    range: selection.range.start + delta..selection.range.end + delta,
                    reversed: selection.reversed,
                });
            }
        }
        self.apply_row_edits(edits, moved, Vec::new());
    }

    /// Deletes each block's rows with their line break (for the last row,
    /// the break before it). Each selection becomes a caret on the row that
    /// takes the block's place, at its head's display column. A folded
    /// range counts as one row.
    pub fn delete_rows(&mut self) {
        self.sync_display();
        let blocks = self.blocks(true);
        let text = self.text();
        let last_row = text.row_count() - 1;
        let selections = self.selections();
        let mut edits = Vec::new();
        let mut carets = Vec::with_capacity(selections.len());
        let mut delta = 0;
        for block in &blocks {
            let start = text.row_start(block.first());
            // The range deleted, and the row that takes the block's place
            // with where it will start.
            let (range, next) = if block.last() < last_row {
                let end = text.row_start(block.last() + 1);
                (start..end, Some((block.last() + 1, start - delta)))
            } else if block.first() > 0 {
                let above = block.first() - 1;
                let next = (above, text.row_start(above) - delta);
                (start - 1..text.len(), Some(next))
            } else {
                (0..text.len(), None)
            };
            delta += range.len();
            edits.push((range, String::new()));
            for selection in &selections[block.selections.clone()] {
                let Some((row, row_start)) = next else {
                    carets.push(caret(0));
                    continue;
                };
                let head = selection.head();
                let head_row = text.offset_to_point(head).row;
                let from = self.expanded_row(head_row);
                let display = from.to_display(head - text.row_start(head_row));
                let column = from.char_column(display);
                let to = self.expanded_row(row);
                let col = to.to_buffer(to.display_col_for_char_column(column), Bias::Left);
                carets.push(caret(row_start + col));
            }
        }
        self.apply_row_edits(edits, carets, Vec::new());
    }

    /// Applies `edits`, which each insert at or delete from a row's start,
    /// mapping the selections across them: an offset at an insertion moves
    /// past it, and one in a deletion goes to its start.
    fn edit_rows_in_place(&mut self, edits: Vec<(Range<usize>, String)>) {
        let selections = self
            .selections()
            .into_iter()
            .map(|selection| ResolvedSelection {
                range: map_offset(&edits, selection.range.start)
                    ..map_offset(&edits, selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
    }

    /// Applies `edits` as one transaction and sets `selections`, offsets in
    /// the text they make, then folds `folds`, rows of that text.
    fn apply_row_edits(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        selections: Vec<ResolvedSelection>,
        folds: Vec<RangeInclusive<u32>>,
    ) {
        if edits.is_empty() {
            return;
        }
        // Anchored before catching up, whose measuring may edit the buffer
        // through another model.
        let (selections, version) = {
            let mut buffer = self.buffer.borrow_mut();
            let Some(id) = buffer.edit(edits, self.origin.clone()) else {
                return;
            };
            if let Some(transactions) = &mut self.transactions {
                transactions.push(id);
            }
            let snapshot = buffer.snapshot();
            (
                SelectionSet::from_resolved(&snapshot, selections),
                snapshot.version(),
            )
        };
        self.sync_display();
        self.goals.clear();
        self.selections = selections;
        // Folds are rows of the text right after the edit, so only stand
        // if nothing else came in with it.
        if !folds.is_empty() && self.snapshot.version() == version {
            self.display.fold_many(folds);
        }
    }

    /// Each block of selections, by row. With `whole_folds`, a block
    /// touching a folded range takes in all of it.
    fn blocks(&self, whole_folds: bool) -> Vec<Block> {
        let text = self.text();
        let mut blocks: Vec<Block> = Vec::new();
        for (index, selection) in self.selections().iter().enumerate() {
            let mut rows = touched_rows(text, &selection.range);
            if whole_folds {
                let first = self
                    .display
                    .hidden_rows(*rows.start())
                    .map_or(*rows.start(), |hidden| hidden.start - 1);
                rows = first..=self.fold_end(*rows.end());
            }
            match blocks.last_mut() {
                Some(block) if *rows.start() <= block.last() + 1 => {
                    let first = block.first().min(*rows.start());
                    block.rows = first..=block.last().max(*rows.end());
                    block.selections.end = index + 1;
                }
                _ => blocks.push(Block {
                    rows,
                    selections: index..index + 1,
                }),
            }
        }
        blocks
    }

    /// The last row folded together with `row`: the end of the folded
    /// range it heads or is in, or `row` itself.
    fn fold_end(&self, row: u32) -> u32 {
        if row + 1 >= self.text().row_count() {
            return row;
        }
        self.display
            .hidden_rows(row + 1)
            .map_or(row, |hidden| hidden.end - 1)
    }

    /// Whether any selection touches more than one row.
    pub(crate) fn touches_many_rows(&self) -> bool {
        let text = self.text();
        self.selections().iter().any(|selection| {
            let rows = touched_rows(text, &selection.range);
            rows.start() != rows.end()
        })
    }
}

/// The rows `range` touches: a non-empty range ending at the start of a
/// later row leaves that row out.
fn touched_rows(text: &Text, range: &Range<usize>) -> RangeInclusive<u32> {
    let start = text.offset_to_point(range.start).row;
    let end = text.offset_to_point(range.end);
    let last = if end.row > start && end.column == 0 {
        end.row - 1
    } else {
        end.row
    };
    start..=last
}

/// The text of `rows`, without the line break after the last.
fn rows_text(text: &Text, rows: RangeInclusive<u32>) -> String {
    let start = text.row_start(*rows.start());
    let end = text.row_start(*rows.end()) + text.row_len(*rows.end()) as usize;
    text.slice_to_string(start..end)
}

/// Where `offset` is once `edits`, sorted and disjoint, are applied: past
/// an insertion at it, or at the start of a deletion it's in.
fn map_offset(edits: &[(Range<usize>, String)], offset: usize) -> usize {
    let mut delta = 0isize;
    for (range, new_text) in edits {
        if range.start > offset {
            break;
        }
        if offset < range.end {
            return range.start.saturating_add_signed(delta);
        }
        delta += new_text.len() as isize - range.len() as isize;
    }
    offset.saturating_add_signed(delta)
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
