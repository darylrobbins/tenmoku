//! Move Last Selection to Next Find Match, as VS Code's
//! `editor.action.moveSelectionToNextFindMatch` (cmd-k cmd-d).
//!
//! Like Select Next Occurrence, but the primary selection is replaced by the
//! next match instead of kept: the others stay. What a match is, and the
//! seeding of carets with their words, are Select Next Occurrence's.

use std::ops::Range;

use fe_buffer::{Bias, ResolvedSelection};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
    multi_cursor::{Step, each_match, is_whole_word, overlaps},
    occurrences::MAX_WORD_BYTES,
};

/// Registers `editor.move_last_selection_to_next_find_match`, which doesn't
/// edit.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "move_last_selection_to_next_find_match",
        "Move Last Selection to Next Find Match",
        false,
        |model, _| {
            model.move_last_selection_to_next_find_match();
            Ok(Effect::Changed)
        },
    );
}

impl EditorModel {
    /// With a caret as the primary selection, selects the word around each
    /// caret. Otherwise replaces the primary selection with the next match of its text after it,
    /// wrapping once, that overlaps no selection; whole words only if it is
    /// one. Nothing if there is no such match, or the text is over
    /// [`MAX_WORD_BYTES`].
    pub fn move_last_selection_to_next_find_match(&mut self) {
        self.sync_display();
        let primary = self.primary_selection();
        if primary.range.is_empty() {
            // Over the limit, the word is no target: leave the carets be.
            if self.word_range_at(primary.head()).len() > MAX_WORD_BYTES {
                return;
            }
            self.seed_carets();
            return;
        }
        if primary.range.len() > MAX_WORD_BYTES {
            return;
        }
        let Some((needle, whole_word)) = self.needle() else {
            return;
        };
        let n = needle.len();
        let selections = self.selections();
        let text = self.text();
        let from = selections.last().expect("one selection").range.end;
        let first_in = |range: Range<usize>| {
            let mut found = None;
            each_match(text, needle.as_bytes(), range, |offset| {
                if overlaps(&selections, offset..offset + n)
                    || (whole_word && !is_whole_word(text, offset..offset + n))
                {
                    return Step::Pass;
                }
                found = Some(offset);
                Step::Stop
            });
            found
        };
        // Wrapping, matches starting from `from` were all looked at already.
        let found = first_in(from..text.len())
            .or_else(|| first_in(0..text.clip_offset(from + n - 1, Bias::Right)));
        let Some(offset) = found else {
            return;
        };
        let mut moved = selections;
        moved.pop();
        moved.push(ResolvedSelection {
            range: offset..offset + n,
            reversed: false,
        });
        self.set_selections(moved);
    }
}

#[cfg(test)]
#[path = "move_to_match_tests.rs"]
mod tests;
