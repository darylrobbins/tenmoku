//! Row insertion, as VS Code's Insert Line Below / Above and Copy Lines Up
//! (`editor.action.insertLineAfter`, `insertLineBefore`, `copyLinesUpAction`).
//!
//! Insert Below opens a row under the last row each selection touches, as
//! pressing enter at that row's end would, so the host's auto-indent
//! applies; Insert Above opens one over the first, carrying that row's
//! indentation. Selections on the same row make one row. Copy Rows Up is
//! [`EditorModel::duplicate_rows`] the other way round: the copy goes above
//! and the selections stay on it.

use fe_buffer::ResolvedSelection;
use fe_command::{Effect, Registry};

use crate::{
    EditorModel, caret,
    commands::{Editor, add},
    rows::touched_rows,
};

/// Registers the row insertion commands. Each edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "insert_line_below",
        "Insert Line Below",
        true,
        |model, host| {
            model.insert_row_below(host);
            Ok(Effect::Changed)
        },
    );
    add(
        registry,
        "insert_line_above",
        "Insert Line Above",
        true,
        |model, _| {
            model.insert_row_above();
            Ok(Effect::Changed)
        },
    );
    add(
        registry,
        "copy_rows_up",
        "Copy Rows Up",
        true,
        |model, _| {
            model.copy_rows_up();
            Ok(Effect::Changed)
        },
    );
}

impl EditorModel {
    /// Opens an empty row below the last row each selection touches, with
    /// a caret on it, indented as a line break at that row's end would be
    /// ([`EditorModel::newline_in`]). One undo step.
    pub fn insert_row_below(&mut self, host: &dyn crate::commands::EditorHost) {
        self.sync_display();
        let text = self.text();
        let ends: Vec<_> = self
            .selections()
            .iter()
            .map(|selection| {
                let row = *touched_rows(text, &selection.range).end();
                caret(text.row_start(row) + text.row_len(row) as usize)
            })
            .collect();
        self.set_selections(ends);
        self.newline_in(host);
    }

    /// Opens an empty row above the first row each selection touches, with
    /// that row's indentation and a caret at its end. One undo step.
    pub fn insert_row_above(&mut self) {
        self.sync_display();
        let text = self.text();
        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let mut carets = Vec::new();
        let mut delta = 0;
        for selection in self.selections() {
            let row = *touched_rows(text, &selection.range).start();
            let start = text.row_start(row);
            if edits.last().is_some_and(|(range, _)| range.start == start) {
                continue;
            }
            let span = self.row_span(start);
            let indent = text.slice_to_string(span.start..span.start + self.indent_len(&span));
            carets.push(caret(start + delta + indent.len()));
            delta += indent.len() + 1;
            edits.push((start..start, format!("{indent}\n")));
        }
        self.apply_row_edits(edits, carets, Vec::new());
    }

    /// Inserts a copy of each block's rows above it, leaving its selections
    /// on the upper copy. A folded range counts as one row.
    pub fn copy_rows_up(&mut self) {
        self.sync_display();
        let blocks = self.blocks(true);
        let text = self.text();
        let mut edits = Vec::new();
        // Each block's copy's length, with the row it's above.
        let mut copies = Vec::new();
        for block in &blocks {
            let first = *block.rows.start();
            let last = *block.rows.end();
            let start = text.row_start(first);
            let end = text.row_start(last) + text.row_len(last) as usize;
            let copy = text.slice_to_string(start..end);
            copies.push((last, copy.len() + 1));
            edits.push((start..start, format!("{copy}\n")));
        }
        let moved = self
            .selections()
            .into_iter()
            .map(|selection| {
                let row = text.offset_to_point(selection.range.start).row;
                let delta: usize = copies
                    .iter()
                    .take_while(|(last, _)| *last < row)
                    .map(|(_, len)| len)
                    .sum();
                ResolvedSelection {
                    range: selection.range.start + delta..selection.range.end + delta,
                    reversed: selection.reversed,
                }
            })
            .collect();
        self.apply_row_edits(edits, moved, Vec::new());
    }
}

#[cfg(test)]
#[path = "row_insert_tests.rs"]
mod tests;
