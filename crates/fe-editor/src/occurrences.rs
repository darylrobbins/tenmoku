//! Highlights the other uses of the word at the caret: a
//! [`DecorationProvider`] that paints every whole-word, case-sensitive
//! occurrence of its target word. Headless: the host picks the target (from
//! [`EditorModel::occurrence_target`]) and repaints when it changes.

use std::ops::Range;

use fe_buffer::{Anchor, Bias, BufferSnapshot};

use crate::{
    CharClass, EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
    multi_cursor::{Step, each_match, is_whole_word},
};

/// The most highlights one [`DecorationProvider::decorations`] call returns.
pub const MAX_HIGHLIGHTS: usize = 1_000;

/// The longest word, in bytes, that is a target.
pub const MAX_WORD_BYTES: usize = 256;

const PRIORITY: i32 = -50;

struct Target {
    word: String,
    range: Range<Anchor>,
}

/// See the [module docs](self).
#[derive(Default)]
pub struct OccurrenceHighlights {
    target: Option<Target>,
}

impl OccurrenceHighlights {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the word at `target` (in `snapshot`) the one to highlight, or
    /// nothing. A word over [`MAX_WORD_BYTES`] is no target. Returns whether
    /// the target changed: the word's text or where it is.
    pub fn set_target(&mut self, snapshot: &BufferSnapshot, target: Option<Range<usize>>) -> bool {
        let text = snapshot.text();
        let new = target
            .filter(|range| !range.is_empty() && range.len() <= MAX_WORD_BYTES)
            .filter(|range| range.end <= text.len())
            .map(|range| Target {
                word: text.slice_to_string(range.clone()),
                range: snapshot.anchor_at(range.start, Bias::Right)
                    ..snapshot.anchor_at(range.end, Bias::Left),
            });
        let changed = match (&self.target, &new) {
            (None, None) => false,
            (Some(old), Some(new)) => {
                old.word != new.word
                    || snapshot.resolve_range(&old.range) != snapshot.resolve_range(&new.range)
            }
            _ => true,
        };
        self.target = new;
        changed
    }
}

impl DecorationProvider for OccurrenceHighlights {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let Some(target) = &self.target else {
            return;
        };
        let text = snapshot.text();
        // Edited since: the word at the anchors is no longer the target.
        let own = snapshot.resolve_range(&target.range);
        if own.end > text.len() || text.slice_to_string(own) != target.word {
            return;
        }
        let n = target.word.len();
        let from = text.clip_offset(range.start.saturating_sub(n), Bias::Left);
        let to = text.clip_offset(range.end.saturating_add(n).min(text.len()), Bias::Right);
        let mut count = 0;
        each_match(text, target.word.as_bytes(), from..to, |offset| {
            let found = offset..offset + n;
            if !overlaps(&found, &range) || !is_whole_word(text, found.clone()) {
                return Step::Pass;
            }
            out.push(
                Decoration::new(found, DecorationKind::Highlight, Tint::Muted)
                    .with_priority(PRIORITY),
            );
            count += 1;
            if count == MAX_HIGHLIGHTS {
                Step::Stop
            } else {
                Step::Take
            }
        });
    }
}

impl EditorModel {
    /// The word to highlight other uses of: with exactly one selection, the
    /// word around a caret (or just before it), or a selection of exactly one
    /// whole word on one row. `None` for several selections, punctuation,
    /// whitespace, a selection over rows or one of part of a word.
    pub fn occurrence_target(&self) -> Option<Range<usize>> {
        let selections = self.selections();
        let [selection] = selections.as_slice() else {
            return None;
        };
        let range = &selection.range;
        let word = self.word_range_at(range.start);
        if !range.is_empty() && *range != word {
            return None;
        }
        let text = self.text();
        let is_word = !word.is_empty()
            && text
                .slice_to_string(word.clone())
                .chars()
                .all(|ch| CharClass::of(ch) == CharClass::Word);
        is_word.then_some(word)
    }
}

#[cfg(test)]
#[path = "occurrences_tests.rs"]
mod tests;
