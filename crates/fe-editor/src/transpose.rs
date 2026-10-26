//! Transpose Letters, as VS Code's `editor.action.transposeLetters`.
//!
//! Each empty selection swaps the grapheme before the caret with the one
//! after it and leaves the caret after the pair; at a row's end it swaps the
//! two before it. Selections with text are left alone. All the swaps are one
//! transaction; a caret whose pair overlaps the previous caret's is dropped.
//!
//! Divergences from VS Code, which works in UTF-16 units and, off the first
//! row, at column 0 swaps across the row break (moving the line break): here
//! a letter is a grapheme, and a caret at column 0 or on a row of fewer than
//! two graphemes does nothing, so the row break is never moved.

use std::ops::Range;

use fe_command::{Effect, Registry};

use crate::{
    EditorModel, caret,
    commands::{Editor, add},
};

/// Registers the transpose command. It edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "transpose_letters",
        "Transpose Letters",
        true,
        |model, _| {
            Ok(if model.transpose_letters() {
                Effect::Changed
            } else {
                Effect::Nothing
            })
        },
    );
}

impl EditorModel {
    /// Transposes at each caret. Returns whether any pair was swapped.
    pub fn transpose_letters(&mut self) -> bool {
        self.sync_display();
        let text = self.text();
        let selections = self.selections();
        // Each caret's pair as (first letter, second letter), by caret.
        let mut pairs: Vec<Option<(Range<usize>, Range<usize>)>> = selections
            .iter()
            .map(|selection| {
                let offset = selection.range.start;
                if !selection.range.is_empty() {
                    return None;
                }
                let row = self.row_span(offset);
                if offset == row.start {
                    return None;
                }
                let end = if offset >= row.end {
                    row.end
                } else {
                    self.next_grapheme(offset)
                };
                let middle = self.prev_grapheme(end);
                if middle <= row.start {
                    return None;
                }
                let begin = self.prev_grapheme(middle);
                Some((begin..middle, middle..end))
            })
            .collect();
        // Drop a pair that overlaps the one before it.
        let mut order: Vec<usize> = (0..pairs.len()).filter(|&i| pairs[i].is_some()).collect();
        order.sort_by_key(|&i| pairs[i].as_ref().map(|(first, _)| first.start));
        let mut last_end = 0;
        for index in order {
            let (first, second) = pairs[index].clone().unwrap();
            if first.start < last_end {
                pairs[index] = None;
            } else {
                last_end = second.end;
            }
        }
        let mut edits: Vec<(Range<usize>, String)> = pairs
            .iter()
            .flatten()
            .map(|(first, second)| {
                let swapped =
                    text.slice_to_string(second.clone()) + &text.slice_to_string(first.clone());
                (first.start..second.end, swapped)
            })
            .collect();
        edits.sort_by_key(|(range, _)| range.start);
        let carets = selections
            .iter()
            .zip(&pairs)
            .map(|(selection, pair)| match pair {
                Some((_, second)) => caret(second.end),
                None => selection.clone(),
            })
            .collect();
        let changed = !edits.is_empty();
        // A swap keeps the text's length, so offsets outside it stay valid.
        self.apply_row_edits(edits, carets, Vec::new());
        changed
    }
}

#[cfg(test)]
#[path = "transpose_tests.rs"]
mod tests;
