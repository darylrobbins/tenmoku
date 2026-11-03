//! Moving and deleting by the parts of a name, as VS Code's `cursorWordPart*`
//! and `deleteWordPart*`: `fooBar`, `HTTPServer`, `utf8Decode` and
//! `snake_case` have two or three stops where [`EditorModel::next_word`] has
//! one.
//!
//! A part ends inside a run of letters and digits at lower to upper case
//! (`foo|Bar`), before the last capital of a run of them that a lowercase
//! letter follows (`HTTP|Server`), and between a letter and a digit
//! (`utf|8|Decode`). A letter with no case splits from neither case.
//! `_` is skipped on the way to a part, as whitespace is, so a move stops on
//! the side of each `_` run it reaches first: right gives `snake|_case|`, left
//! `|snake_|case`. Whitespace and other runs behave as in a word move.

use std::ops::Range;

use crate::{CharClass, EditorModel, chars_backward, chars_forward};

/// What a char is to the splitting of a run of letters and digits.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Part {
    Digit,
    Upper,
    Lower,
    Uncased,
}

impl Part {
    fn of(ch: char) -> Self {
        if ch.is_numeric() {
            Part::Digit
        } else if ch.is_uppercase() {
            Part::Upper
        } else if ch.is_lowercase() {
            Part::Lower
        } else {
            Part::Uncased
        }
    }
}

/// Whether a part ends between the neighbours `left` and `right`, with `after`
/// the char following `right`. It's a question of the text, not of the
/// direction: both moves stop where this says.
fn splits(left: Part, right: Part, after: Option<Part>) -> bool {
    match (left, right) {
        (Part::Digit, Part::Digit) => false,
        (Part::Digit, _) | (_, Part::Digit) | (Part::Lower, Part::Upper) => true,
        (Part::Upper, Part::Upper) => after == Some(Part::Lower),
        _ => false,
    }
}

/// Whether `ch` can be in a part: a word char other than `_`.
fn in_part(ch: char) -> bool {
    ch != '_' && CharClass::of(ch) == CharClass::Word
}

impl EditorModel {
    /// Where a subword move right from `offset` lands. Row-bounded as
    /// [`EditorModel::next_word`] is: at a row's end it steps over the break.
    pub(crate) fn next_subword(&self, offset: usize) -> usize {
        let row = self.row_span(offset);
        if offset >= row.end {
            return (offset + 1).min(self.text().len());
        }
        let mut chars = chars_forward(self.text(), offset..row.end).peekable();
        let mut end = offset;
        while let Some(&(ix, ch)) = chars.peek()
            && ch.is_whitespace()
        {
            end = ix + ch.len_utf8();
            chars.next();
        }
        if let Some(&(_, first)) = chars.peek()
            && CharClass::of(first) == CharClass::Punctuation
        {
            while let Some(&(ix, ch)) = chars.peek()
                && CharClass::of(ch) == CharClass::Punctuation
            {
                end = ix + ch.len_utf8();
                chars.next();
            }
            return end;
        }
        while let Some(&(ix, '_')) = chars.peek() {
            end = ix + 1;
            chars.next();
        }
        let mut part = chars.take_while(|&(_, ch)| in_part(ch));
        let Some((ix, ch)) = part.next() else {
            return end;
        };
        let mut end = ix + ch.len_utf8();
        let mut left = Part::of(ch);
        let mut right = part.next();
        while let Some((ix, ch)) = right {
            let after = part.next();
            let part_of_right = Part::of(ch);
            if splits(left, part_of_right, after.map(|(_, ch)| Part::of(ch))) {
                break;
            }
            end = ix + ch.len_utf8();
            left = part_of_right;
            right = after;
        }
        end
    }

    /// Where a subword move left from `offset` lands: the other way along
    /// the row from [`EditorModel::next_subword`], stepping over the break
    /// at a row's start.
    pub(crate) fn prev_subword(&self, offset: usize) -> usize {
        let row = self.row_span(offset);
        if offset == row.start {
            return offset.saturating_sub(1);
        }
        let mut chars = chars_backward(self.text(), row.start..offset).peekable();
        let mut start = offset;
        while let Some(&(ix, ch)) = chars.peek()
            && ch.is_whitespace()
        {
            start = ix;
            chars.next();
        }
        if let Some(&(_, first)) = chars.peek()
            && CharClass::of(first) == CharClass::Punctuation
        {
            while let Some(&(ix, ch)) = chars.peek()
                && CharClass::of(ch) == CharClass::Punctuation
            {
                start = ix;
                chars.next();
            }
            return start;
        }
        // From inside a part the char after the first one read is past the
        // caret: `HTTPS|erver` is still `HTTP|Server`.
        let mut after = (start == offset)
            .then(|| chars_forward(self.text(), offset..row.end).next())
            .flatten()
            .map(|(_, ch)| Part::of(ch));
        while let Some(&(ix, '_')) = chars.peek() {
            start = ix;
            after = None;
            chars.next();
        }
        let mut part = chars.take_while(|&(_, ch)| in_part(ch));
        let Some((ix, ch)) = part.next() else {
            return start;
        };
        let mut start = ix;
        let mut right = Part::of(ch);
        for (ix, ch) in part {
            let left = Part::of(ch);
            if splits(left, right, after) {
                break;
            }
            start = ix;
            after = Some(right);
            right = left;
        }
        start
    }

    /// Deletes each selection, or from each caret to where Move Subword Left
    /// goes.
    pub fn delete_subword_left(&mut self) {
        self.delete_subwords(|model, offset| model.prev_subword(offset)..offset);
    }

    /// Deletes each selection, or from each caret to where Move Subword Right
    /// goes.
    pub fn delete_subword_right(&mut self) {
        self.delete_subwords(|model, offset| offset..model.next_subword(offset));
    }

    /// Deletes each selection, or for a caret the range `around` gives, each
    /// widened to whole graphemes: a subword move or select can stop inside a
    /// cluster, at its combining mark, so a selection can end there as a
    /// caret can. One undo step.
    fn delete_subwords(&mut self, around: impl Fn(&Self, usize) -> Range<usize>) {
        self.sync_display();
        let edits = self
            .selections()
            .into_iter()
            .map(|selection| {
                let range = if selection.range.is_empty() {
                    around(self, selection.range.start)
                } else {
                    selection.range
                };
                (self.whole_graphemes(range), String::new())
            })
            .collect();
        self.apply_typed(edits, None);
    }
}

#[cfg(test)]
#[path = "subword_tests.rs"]
mod tests;
