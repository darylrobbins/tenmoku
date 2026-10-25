//! Trim Trailing Whitespace, as VS Code's `editor.action.trimTrailingWhitespace`.
//!
//! Removes the spaces and tabs at the end of every row of the document. On a
//! row holding an empty caret, the whitespace up to the caret stays and only
//! what follows it goes, so typing a space and trimming doesn't eat the
//! space. The command makes its edits as one transaction.

use std::ops::Range;

use fe_buffer::{ResolvedSelection, Text};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
    join_lines::map_before,
};

/// Registers the trim command. It edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "trim_trailing_whitespace",
        "Trim Trailing Whitespace",
        true,
        |model, _| {
            model.trim_trailing_whitespace();
            Ok(Effect::Changed)
        },
    );
}

/// The spaces and tabs at the end of each row of `text`, as sorted, disjoint
/// ranges; rows with none have none. Whitespace before an offset of `keep`
/// on its row (an empty caret's) stays.
pub fn trailing_whitespace(text: &Text, keep: &[usize]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for row in 0..text.row_count() {
        let start = text.row_start(row);
        let len = text.row_len(row) as usize;
        let content = text.slice_to_string(start..start + len);
        let mut kept = content.trim_end_matches([' ', '\t']).len();
        if kept == len {
            continue;
        }
        let end = start + len;
        // The last caret on the row decides how much stays.
        let last = keep
            .iter()
            .filter(|&&caret| caret >= start && caret <= end)
            .max();
        if let Some(&caret) = last {
            kept = kept.max(caret - start);
        }
        if kept < len {
            ranges.push(start + kept..end);
        }
    }
    ranges
}

impl EditorModel {
    /// Trims the trailing whitespace of every row, keeping what an empty
    /// caret has before it. Selections stay over their text.
    pub fn trim_trailing_whitespace(&mut self) {
        self.sync_display();
        let all = self.selections();
        let carets: Vec<usize> = all
            .iter()
            .filter(|selection| selection.range.is_empty())
            .map(|selection| selection.range.start)
            .collect();
        let edits: Vec<(Range<usize>, String)> = trailing_whitespace(self.text(), &carets)
            .into_iter()
            .map(|range| (range, String::new()))
            .collect();
        let selections = all
            .iter()
            .map(|selection| ResolvedSelection {
                range: map_before(&edits, selection.range.start)
                    ..map_before(&edits, selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
    }
}

#[cfg(test)]
#[path = "trim_whitespace_tests.rs"]
mod tests;
