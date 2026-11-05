// SPDX-License-Identifier: Apache-2.0

//! A headless single-line text-editing model: the state behind a text
//! field, without GPUI, so its editing rules are tested without a window.
//!
//! [`TextEdit`] holds the text, one [`Selection`] and an IME composition
//! (the marked range). Offsets are UTF-8 byte offsets. The selection's ends
//! always sit on grapheme-cluster boundaries, so the caret never splits an
//! emoji or a letter from its accents; the marked range sits on char
//! boundaries, since an input method may compose part of a cluster.
//! Platforms speak UTF-16 to input methods: [`TextEdit::offset_to_utf16`]
//! and [`TextEdit::offset_from_utf16`] convert.
//!
//! Words, for the word motions and [`TextEdit::delete_word_left`], are runs
//! of graphemes of one class: word characters (alphanumerics and `_`),
//! punctuation, or whitespace. Moving a word left skips whitespace, then the
//! run before it, so in `src/main.rs` the steps stop at `rs`, `.`, `main`,
//! `/` and `src`.
//!
//! Each edit is an undo step, except that consecutive typing (and IME
//! composition, which ends in typing) coalesces into one, as do consecutive
//! backspaces and consecutive forward deletes, until the caret moves.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

/// The most undo steps kept.
const UNDO_LIMIT: usize = 200;

/// A selection: `anchor` stays put while `head` (the caret) moves. Both are
/// byte offsets on grapheme boundaries; equal, they're a caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    /// A caret at `offset`.
    pub fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    /// The selected bytes, start first.
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Whether the caret is at the start: the selection was made leftwards.
    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }
}

/// Where a motion takes the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// One grapheme left.
    Left,
    /// One grapheme right.
    Right,
    /// To the start of the word before the caret.
    WordLeft,
    /// To the end of the word after the caret.
    WordRight,
    /// To the start of the text.
    Home,
    /// To the end of the text.
    End,
}

/// A grapheme's class, for words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punctuation,
}

fn class(grapheme: &str) -> Class {
    match grapheme.chars().next() {
        Some(ch) if ch.is_whitespace() => Class::Space,
        Some(ch) if ch.is_alphanumeric() || ch == '_' => Class::Word,
        _ => Class::Punctuation,
    }
}

/// Which edits coalesce into one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    /// Typing and IME composition.
    Typing,
    Backspace,
    Delete,
    /// Never coalesces: paste, cut, delete word, replace.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    selection: Selection,
}

/// A single line of editable text with a selection, IME composition and
/// undo. See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct TextEdit {
    text: String,
    selection: Selection,
    marked: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last edit's kind and the selection it left, while nothing has
    /// moved the selection since: the next edit of that kind coalesces.
    last_edit: Option<(EditKind, Selection)>,
}

impl TextEdit {
    pub fn new() -> Self {
        Self::default()
    }

    /// `text` (line breaks made spaces), with the caret at its end.
    pub fn with_text(text: &str) -> Self {
        let mut edit = Self::new();
        edit.set_text(text);
        edit
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// The selected bytes, start first.
    pub fn selected_range(&self) -> Range<usize> {
        self.selection.range()
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.range()]
    }

    /// The caret: the selection's head.
    pub fn caret(&self) -> usize {
        self.selection.head
    }

    /// The text an input method is composing, if any.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Replaces the whole text (line breaks made spaces), puts the caret at
    /// its end and forgets the undo history: for a host setting the value.
    pub fn set_text(&mut self, text: &str) {
        self.text = single_line(text);
        self.selection = Selection::caret(self.text.len());
        self.marked = None;
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
    }

    // Selection.

    /// Selects `anchor..head`, each clamped to the text and moved back to a
    /// grapheme boundary.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        let selection = Selection {
            anchor: self.boundary_at_or_before(anchor),
            head: self.boundary_at_or_before(head),
        };
        self.select(selection);
    }

    /// Puts the caret at `offset` (clamped and snapped like
    /// [`TextEdit::set_selection`]).
    pub fn move_caret_to(&mut self, offset: usize) {
        self.set_selection(offset, offset);
    }

    /// Moves the caret to `offset`, keeping the anchor: a shift-click.
    pub fn select_to(&mut self, offset: usize) {
        self.set_selection(self.selection.anchor, offset);
    }

    pub fn select_all(&mut self) {
        self.select(Selection {
            anchor: 0,
            head: self.text.len(),
        });
    }

    /// Moves the caret by `motion`, collapsing the selection. With a
    /// selection, left and right go to its start and end.
    pub fn move_by(&mut self, motion: Motion) {
        let range = self.selection.range();
        let target = match motion {
            Motion::Left if !range.is_empty() => range.start,
            Motion::Right if !range.is_empty() => range.end,
            _ => self.target(motion),
        };
        self.select(Selection::caret(target));
    }

    /// Moves the caret by `motion`, keeping the anchor.
    pub fn select_by(&mut self, motion: Motion) {
        let head = self.target(motion);
        self.select(Selection {
            anchor: self.selection.anchor,
            head,
        });
    }

    /// The word, run of punctuation or run of whitespace at `offset`: the
    /// grapheme after it, or before it at the end of the text. For a
    /// double click.
    pub fn word_range_at(&self, offset: usize) -> Range<usize> {
        let offset = self.boundary_at_or_before(offset);
        let graphemes: Vec<(usize, &str)> = self.text.grapheme_indices(true).collect();
        let Some(ix) = graphemes
            .iter()
            .position(|(start, _)| *start >= offset)
            .or_else(|| graphemes.len().checked_sub(1))
        else {
            return 0..0;
        };
        let ix = if graphemes[ix].0 > offset { ix - 1 } else { ix };
        let kind = class(graphemes[ix].1);
        let mut start = ix;
        while start > 0 && class(graphemes[start - 1].1) == kind {
            start -= 1;
        }
        let mut end = ix + 1;
        while end < graphemes.len() && class(graphemes[end].1) == kind {
            end += 1;
        }
        let end = graphemes.get(end).map_or(self.text.len(), |(at, _)| *at);
        graphemes[start].0..end
    }

    // Editing. Each returns whether the text changed.

    /// Types `text`: it replaces the composition if there is one, else the
    /// selection. Line breaks become spaces. Consecutive typing is one undo
    /// step.
    pub fn insert(&mut self, text: &str) -> bool {
        let range = self.marked.clone().unwrap_or_else(|| self.selected_range());
        self.replace(range, text, EditKind::Typing)
    }

    /// Like [`TextEdit::insert`], but always its own undo step: for a paste.
    pub fn paste(&mut self, text: &str) -> bool {
        let range = self.marked.clone().unwrap_or_else(|| self.selected_range());
        self.replace(range, text, EditKind::Other)
    }

    /// Replaces the bytes `range` (clamped to char boundaries) with `text`,
    /// ending the composition and putting the caret after `text`.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str) -> bool {
        let range = self.char_range(range);
        self.replace(range, text, EditKind::Other)
    }

    /// Deletes the selection, or the grapheme before the caret.
    pub fn backspace(&mut self) -> bool {
        let range = self.selected_range();
        if range.is_empty() {
            let start = self.target(Motion::Left);
            self.replace(start..range.end, "", EditKind::Backspace)
        } else {
            self.replace(range, "", EditKind::Other)
        }
    }

    /// Deletes the selection, or the grapheme after the caret.
    pub fn delete(&mut self) -> bool {
        let range = self.selected_range();
        if range.is_empty() {
            let end = self.target(Motion::Right);
            self.replace(range.start..end, "", EditKind::Delete)
        } else {
            self.replace(range, "", EditKind::Other)
        }
    }

    /// Deletes the selection, or back to the start of the word before the
    /// caret.
    pub fn delete_word_left(&mut self) -> bool {
        let range = self.selected_range();
        let start = if range.is_empty() {
            self.target(Motion::WordLeft)
        } else {
            range.start
        };
        self.replace(start..range.end, "", EditKind::Other)
    }

    /// Deletes the selection, or up to the end of the word after the caret.
    pub fn delete_word_right(&mut self) -> bool {
        let range = self.selected_range();
        let end = if range.is_empty() {
            self.target(Motion::WordRight)
        } else {
            range.end
        };
        self.replace(range.start..end, "", EditKind::Other)
    }

    /// Deletes the selected text and returns it: for a cut.
    pub fn cut(&mut self) -> Option<String> {
        let range = self.selected_range();
        if range.is_empty() {
            return None;
        }
        let text = self.text[range.clone()].to_string();
        self.replace(range, "", EditKind::Other);
        Some(text)
    }

    /// Deletes all the text.
    pub fn clear(&mut self) -> bool {
        self.replace(0..self.text.len(), "", EditKind::Other)
    }

    // IME composition.

    /// Replaces `range` (or the composition, or the selection) with `text`
    /// and marks it as being composed; `selected`, relative to `text`, is
    /// where the caret goes inside it (its end if `None`). Empty `text`
    /// ends the composition.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> bool {
        let range = match range {
            Some(range) => self.char_range(range),
            None => self.marked.clone().unwrap_or_else(|| self.selected_range()),
        };
        let text = single_line(text);
        let changed = self.replace(range.clone(), &text, EditKind::Typing);
        if text.is_empty() {
            return changed;
        }
        let marked = range.start..range.start + text.len();
        self.marked = Some(marked.clone());
        if let Some(selected) = selected {
            let clamp = |offset: usize| {
                let mut offset = marked.start + offset.min(text.len());
                while !self.text.is_char_boundary(offset) {
                    offset -= 1;
                }
                offset
            };
            let selection = Selection {
                anchor: self.boundary_at_or_before(clamp(selected.start)),
                head: self.boundary_at_or_before(clamp(selected.end)),
            };
            self.selection = selection;
            self.last_edit = Some((EditKind::Typing, selection));
        }
        changed
    }

    /// Ends the composition, keeping its text.
    pub fn unmark(&mut self) {
        self.marked = None;
    }

    // Undo.

    /// Undoes the last undo step; returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        let current = self.restore(snapshot);
        self.redo.push(current);
        true
    }

    /// Redoes the last undone step; returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        let current = self.restore(snapshot);
        self.undo.push(current);
        true
    }

    // UTF-16.

    /// The UTF-16 offset of byte `offset` (clamped to the text).
    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (ix, ch) in self.text.char_indices() {
            if ix >= offset {
                break;
            }
            utf16 += ch.len_utf16();
        }
        utf16
    }

    /// The byte offset of UTF-16 offset `utf16`, clamped to the text; an
    /// offset inside a surrogate pair goes to the char's end.
    pub fn offset_from_utf16(&self, utf16: usize) -> usize {
        let mut seen = 0;
        for (ix, ch) in self.text.char_indices() {
            if seen >= utf16 {
                return ix;
            }
            seen += ch.len_utf16();
        }
        self.text.len()
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range.start);
        start..self.offset_from_utf16(range.end).max(start)
    }

    /// The text's length in UTF-16 code units.
    pub fn len_utf16(&self) -> usize {
        self.text.encode_utf16().count()
    }

    // Internals.

    fn select(&mut self, selection: Selection) {
        if selection != self.selection {
            self.last_edit = None;
        }
        self.selection = selection;
        self.marked = None;
    }

    /// Replaces `range` (on char boundaries) with `text`, recording an undo
    /// step unless the edit coalesces with the last.
    fn replace(&mut self, range: Range<usize>, text: &str, kind: EditKind) -> bool {
        let text = single_line(text);
        if range.is_empty() && text.is_empty() {
            self.marked = None;
            return false;
        }
        let coalesces = kind != EditKind::Other
            && self.last_edit == Some((kind, self.selection))
            && !self.undo.is_empty();
        if !coalesces {
            self.undo.push(Snapshot {
                text: self.text.clone(),
                selection: self.selection,
            });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.text.replace_range(range.clone(), &text);
        self.marked = None;
        let caret = self.boundary_at_or_after(range.start + text.len());
        self.selection = Selection::caret(caret);
        self.last_edit = Some((kind, self.selection));
        true
    }

    /// Puts `snapshot` in place and returns what it replaced.
    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let current = Snapshot {
            text: std::mem::replace(&mut self.text, snapshot.text),
            selection: self.selection,
        };
        self.selection = snapshot.selection;
        self.marked = None;
        self.last_edit = None;
        current
    }

    /// Where `motion` takes the caret from the head.
    fn target(&self, motion: Motion) -> usize {
        let head = self.selection.head;
        match motion {
            Motion::Home => 0,
            Motion::End => self.text.len(),
            Motion::Left => self.text[..head]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(ix, _)| ix),
            Motion::Right => self.text[head..]
                .graphemes(true)
                .next()
                .map_or(head, |grapheme| head + grapheme.len()),
            Motion::WordLeft => {
                let mut graphemes = self.text[..head].grapheme_indices(true).rev().peekable();
                while graphemes
                    .next_if(|(_, g)| class(g) == Class::Space)
                    .is_some()
                {}
                let Some(&(mut start, first)) = graphemes.peek() else {
                    return 0;
                };
                let kind = class(first);
                while let Some((ix, _)) = graphemes.next_if(|(_, g)| class(g) == kind) {
                    start = ix;
                }
                start
            }
            Motion::WordRight => {
                let mut graphemes = self.text[head..].grapheme_indices(true).peekable();
                while graphemes
                    .next_if(|(_, g)| class(g) == Class::Space)
                    .is_some()
                {}
                let Some(&(_, first)) = graphemes.peek() else {
                    return self.text.len();
                };
                let kind = class(first);
                let mut end = head;
                while let Some((ix, g)) = graphemes.next_if(|(_, g)| class(g) == kind) {
                    end = head + ix + g.len();
                }
                end
            }
        }
    }

    /// The grapheme boundary at or before `offset`, clamped to the text.
    fn boundary_at_or_before(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        self.text
            .grapheme_indices(true)
            .map(|(ix, _)| ix)
            .take_while(|ix| *ix <= offset)
            .last()
            .unwrap_or(0)
    }

    /// The grapheme boundary at or after `offset`, clamped to the text.
    fn boundary_at_or_after(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(ix, _)| ix)
            .find(|ix| *ix >= offset)
            .unwrap_or(self.text.len())
    }

    /// `range` clamped to the text, its ends moved back to char boundaries.
    fn char_range(&self, range: Range<usize>) -> Range<usize> {
        let clamp = |mut offset: usize| {
            offset = offset.min(self.text.len());
            while !self.text.is_char_boundary(offset) {
                offset -= 1;
            }
            offset
        };
        let start = clamp(range.start);
        start..clamp(range.end).max(start)
    }
}

/// `text` on one line: each line break (`\r\n`, `\n` or `\r`) a space.
fn single_line(text: &str) -> String {
    if !text.contains(['\n', '\r']) {
        return text.to_string();
    }
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}
