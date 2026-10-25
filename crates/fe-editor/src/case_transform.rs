//! Transform to Uppercase, Lowercase and Title Case, as VS Code's
//! `editor.action.transformToUppercase`, `transformToLowercase` and
//! `transformToTitlecase`.
//!
//! Each non-empty selection is replaced by its transformed text. An empty
//! selection transforms the word at the caret, and nothing if the caret is
//! not in one; the caret keeps its offset in the word. Case mapping is full
//! Unicode (`ß` uppercases to `SS`), so a selection follows the new length.
//! Title case upper-cases the first letter after whitespace or one of VS
//! Code's `titlecase` expression does: the whole text is lower-cased, then a
//! letter is upper-cased if it starts the text, follows anything but a letter,
//! digit or apostrophe (so `_` starts a word, digits do not), or follows an
//! apostrophe that itself starts the text or follows a non-letter (so `don't`
//! keeps its `t` lower). A selection whose text doesn't change makes no edit.
//! Selections or words that overlap are transformed as one range. All the
//! edits are one transaction.

use std::ops::Range;

use fe_buffer::ResolvedSelection;
use fe_command::{Effect, Registry};
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, GeneralCategoryGroup},
};

use crate::{
    CharClass, EditorModel,
    commands::{Editor, add},
};

/// Whether `ch` is in a Unicode general category of `group`, as a `\p{..}`
/// class matches it.
fn in_group(ch: char, group: GeneralCategoryGroup) -> bool {
    group.contains(CodePointMapData::<GeneralCategory>::new().get(ch))
}

fn is_letter(ch: char) -> bool {
    in_group(ch, GeneralCategoryGroup::Letter)
}

fn is_number(ch: char) -> bool {
    in_group(ch, GeneralCategoryGroup::Number)
}

/// How a command changes the case of text.
#[derive(Clone, Copy)]
enum Case {
    Upper,
    Lower,
    Title,
}

impl Case {
    fn apply(self, text: &str) -> String {
        match self {
            Case::Upper => text.to_uppercase(),
            Case::Lower => text.to_lowercase(),
            Case::Title => title_case(&text.to_lowercase()),
        }
    }
}

/// VS Code's title case step after lower-casing: each match of
/// `/(^|[^\p{L}\p{N}']|((^|\P{L})'))\p{L}/gmu` is upper-cased whole,
/// boundary character included (so U+0345 before a letter becomes U+0399).
/// `^` is the text's start or the position after a line terminator.
fn title_case(lower: &str) -> String {
    let chars: Vec<char> = lower.chars().collect();
    let letter_at = |ix: usize| chars.get(ix).is_some_and(|&ch| is_letter(ch));
    let line_start =
        |ix: usize| ix == 0 || matches!(chars[ix - 1], '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let mut out = String::with_capacity(lower.len());
    let mut ix = 0;
    while ix < chars.len() {
        let ch = chars[ix];
        // Alternatives in the expression's order; the match's length.
        let len = if line_start(ix) && letter_at(ix) {
            1
        } else if (!(is_letter(ch) || is_number(ch) || ch == '\'') || line_start(ix) && ch == '\'')
            && letter_at(ix + 1)
        {
            2
        } else if !is_letter(ch) && chars.get(ix + 1) == Some(&'\'') && letter_at(ix + 2) {
            3
        } else {
            0
        };
        if len == 0 {
            out.push(ch);
            ix += 1;
        } else {
            let matched: String = chars[ix..ix + len].iter().collect();
            out.push_str(&matched.to_uppercase());
            ix += len;
        }
    }
    out
}

/// Registers the case commands. Each edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    for (name, title, case) in [
        ("transform_uppercase", "Transform to Uppercase", Case::Upper),
        ("transform_lowercase", "Transform to Lowercase", Case::Lower),
        (
            "transform_titlecase",
            "Transform to Title Case",
            Case::Title,
        ),
    ] {
        add(registry, name, title, true, move |model, _| {
            model.transform_case(case);
            Ok(Effect::Changed)
        });
    }
}

impl EditorModel {
    fn transform_case(&mut self, case: Case) {
        self.sync_display();
        let text = self.text();
        let selections = self.selections();
        // Each selection's target: itself, or the word at a caret (none if
        // the caret is not in one).
        let targets: Vec<Option<Range<usize>>> = selections
            .iter()
            .map(|selection| {
                let range = &selection.range;
                if !range.is_empty() {
                    return Some(range.clone());
                }
                let word = self.word_range_at(range.start);
                let in_word = text
                    .slice_to_string(word.clone())
                    .chars()
                    .next()
                    .is_some_and(|ch| CharClass::of(ch) == CharClass::Word);
                in_word.then_some(word)
            })
            .collect();
        // Overlapping targets merge into one range, transformed once.
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut sorted: Vec<&Range<usize>> = targets.iter().flatten().collect();
        sorted.sort_by_key(|target| (target.start, target.end));
        for target in sorted {
            match ranges.last_mut() {
                Some(last) if target.start < last.end => last.end = last.end.max(target.end),
                _ => ranges.push(target.clone()),
            }
        }
        // Each range's new text, and where it starts in the new text.
        let mut starts = Vec::with_capacity(ranges.len());
        let mut delta = 0isize;
        let mut edits = Vec::new();
        let mut news = Vec::with_capacity(ranges.len());
        for range in &ranges {
            let old = text.slice_to_string(range.clone());
            let new = case.apply(&old);
            starts.push(range.start.checked_add_signed(delta).unwrap());
            delta += new.len() as isize - old.len() as isize;
            if new != old {
                edits.push((range.clone(), new.clone()));
            }
            news.push(new);
        }
        // Where `offset`, old, is in the new text: through the transformed
        // text before it within a range, else shifted by the ranges before.
        let map = |offset: usize| {
            let mut shift = 0isize;
            for (index, range) in ranges.iter().enumerate() {
                if offset < range.start {
                    break;
                }
                if offset <= range.end {
                    let prefix = case.apply(&text.slice_to_string(range.start..offset));
                    return starts[index] + prefix.len().min(news[index].len());
                }
                shift += news[index].len() as isize - (range.end - range.start) as isize;
            }
            offset.checked_add_signed(shift).unwrap()
        };
        let selections = selections
            .iter()
            .map(|selection| ResolvedSelection {
                range: map(selection.range.start)..map(selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
    }
}

#[cfg(test)]
#[path = "case_transform_tests.rs"]
mod tests;
