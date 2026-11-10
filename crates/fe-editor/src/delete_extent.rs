//! Delete All Left, Delete All Right and Delete Word Right, as VS Code's
//! `deleteAllLeft`, `deleteAllRight` and `deleteWordRight`.
//!
//! Each deletes a non-empty selection as Backspace or Delete does, and for a
//! caret the range named below. At a row's edge the row break goes instead.
//! Carets whose ranges touch merge their deletions; one undo step.

use std::ops::Range;

use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
};

/// Registers the three delete commands. They edit the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "delete_all_left", true, |model, _| {
        model.delete_all_left();
        Ok(Effect::Changed)
    });
    add(registry, "delete_all_right", true, |model, _| {
        model.delete_all_right();
        Ok(Effect::Changed)
    });
    add(registry, "delete_word_right", true, |model, _| {
        model.delete_word_right();
        Ok(Effect::Changed)
    });
}

impl EditorModel {
    /// Deletes from each caret back to the start of its row, or the row
    /// break before it at column 0.
    pub fn delete_all_left(&mut self) {
        self.delete_with(
            |model, offset| {
                let start = model.row_span(offset).start;
                let range = if offset == start {
                    model.prev_grapheme(offset)..offset
                } else {
                    start..offset
                };
                model.whole_graphemes(range)
            },
            None,
        );
    }

    /// Deletes from each caret to the end of its row, or the row break there.
    pub fn delete_all_right(&mut self) {
        self.delete_with(
            |model, offset| {
                let end = model.row_span(offset).end;
                let range = if offset == end {
                    offset..model.next_grapheme(offset)
                } else {
                    offset..end
                };
                model.whole_graphemes(range)
            },
            None,
        );
    }

    /// Deletes from each caret to where Move Word Right goes.
    pub fn delete_word_right(&mut self) {
        self.delete_with(
            |model, offset| model.whole_graphemes(offset..model.next_word(offset)),
            None,
        );
    }

    /// `range` widened to grapheme boundaries: a caret can sit inside a
    /// cluster (word movement stops at a combining mark), and a deletion
    /// must not leave half of one.
    pub(crate) fn whole_graphemes(&self, range: Range<usize>) -> Range<usize> {
        if range.is_empty() {
            return range;
        }
        // The cluster holding `offset` when it is inside one.
        let inside = |offset: usize| {
            let start = self.prev_grapheme(offset);
            let end = self.next_grapheme(start);
            (start < offset && offset < end).then_some(start..end)
        };
        let start = inside(range.start).map_or(range.start, |cluster| cluster.start);
        let end = inside(range.end).map_or(range.end, |cluster| cluster.end);
        start..end
    }
}

#[cfg(test)]
#[path = "delete_extent_tests.rs"]
mod tests;
