//! Closing brackets and quotes as they're typed: typing an opener inserts
//! its closer too, or wraps a selection in the pair; typing a closer that
//! typing inserted moves over it; and backspace in an empty pair typing
//! inserted deletes both halves.

use std::ops::Range;

use fe_buffer::{BufferSnapshot, ResolvedSelection, Version};

use crate::{CharClass, EditorModel, Typed, caret, chars_backward, chars_forward};

/// What a closer may come before for typing its opener to close it,
/// besides whitespace and the row's end.
const CLOSES_BEFORE: &[char] = &[';', ':', '.', ',', '=', '}', ']', ')', '>'];

/// A pair typing inserted, at offsets in the text of `version`. Its closer
/// can be typed over while no edit has touched either half since, both are
/// on one row, and a caret is between them.
#[derive(Clone, Debug)]
pub(crate) struct AutoClosed {
    opener: Range<usize>,
    closer: Range<usize>,
    chars: (char, char),
    version: Version,
}

impl AutoClosed {
    /// A pair whose opener and closer are at `opener` and `closer` in
    /// `snapshot`.
    fn new(
        snapshot: &BufferSnapshot,
        opener: Range<usize>,
        closer: Range<usize>,
        chars: (char, char),
    ) -> Self {
        Self {
            opener,
            closer,
            chars,
            version: snapshot.version(),
        }
    }

    /// The pair carried through the edits made since its version, up to
    /// `snapshot`'s: none if any of them replaced or cut into either half,
    /// even with the same char, or left the halves on different rows.
    /// Text inserted at a half's edges moves it, but leaves it whole.
    fn advanced(&self, snapshot: &BufferSnapshot) -> Option<Self> {
        if self.version < snapshot.oldest_version() {
            return None;
        }
        let (mut opener, mut closer) = (self.opener.clone(), self.closer.clone());
        for batch in snapshot.edits_since(self.version) {
            let touches = |half: &Range<usize>| {
                batch
                    .iter()
                    .any(|edit| edit.old.start < half.end && edit.old.end > half.start)
            };
            if touches(&opener) || touches(&closer) {
                return None;
            }
            let moved = |half: &Range<usize>| {
                let start = batch
                    .iter()
                    .filter(|edit| edit.old.end <= half.start)
                    .fold(half.start, |start, edit| {
                        start + edit.new.len() - edit.old.len()
                    });
                start..start + half.len()
            };
            (opener, closer) = (moved(&opener), moved(&closer));
        }
        let row = |offset| snapshot.text().offset_to_point(offset).row;
        (row(opener.start) == row(closer.start)).then_some(Self {
            opener,
            closer,
            chars: self.chars,
            version: snapshot.version(),
        })
    }
}

/// What typing a char does at one selection.
enum Typing {
    /// Replaces the selection with the char.
    Plain,
    /// Inserts the char and the closer, the caret between them.
    Open(char),
    /// Surrounds the selection with the char and the closer, keeping the
    /// selection on the same text.
    Wrap(char),
    /// Moves over the tracked closer that is the char.
    TypeOver(usize),
}

impl EditorModel {
    /// Whether typing an opener closes it; on by default.
    pub fn auto_close(&self) -> bool {
        self.auto_close
    }

    /// Turns closing typed openers on or off. Off, typing inserts exactly
    /// what's typed, and the pairs typed so far are forgotten.
    pub fn set_auto_close(&mut self, on: bool) {
        self.auto_close = on;
        if !on {
            self.auto_closed.clear();
        }
    }

    /// Types `ch` at every selection, as [`EditorModel::insert`] does,
    /// closing what it opens from `pairs` (opener, closer), unless
    /// auto-closing is off. Each selection on its own:
    ///
    /// - An opener at a caret inserts the pair, with the caret between, if
    ///   the next char is the row's end, whitespace or one of `;:.,=}])>`.
    ///   An opener that is its own closer (a quote) also needs the char
    ///   before not to be a word char nor the same quote.
    /// - An opener with a selection surrounds it, keeping it selected.
    /// - A closer right before a caret, that typing inserted (opening or
    ///   wrapping), that no edit has touched since, that is on one row with
    ///   its opener, and that has had a caret between them ever since, is
    ///   moved over rather than typed again.
    ///
    /// It all lands as one edit, in the run of typing as other typing does.
    pub fn insert_typed(&mut self, ch: char, pairs: &[(char, char)]) {
        let mut buf = [0; 4];
        let typed = &*ch.encode_utf8(&mut buf);
        if !self.auto_close || pairs.is_empty() {
            self.insert(typed);
            return;
        }
        self.sync_display();
        let len = ch.len_utf8();
        let closer = pairs
            .iter()
            .find(|&&(open, _)| open == ch)
            .map(|&(_, close)| close);
        let mut edits = Vec::new();
        let mut selections = Vec::new();
        let mut opened = Vec::new();
        let mut typed_over = Vec::new();
        let mut delta = 0isize;
        for selection in self.selections() {
            let range = selection.range.clone();
            let start = range
                .start
                .checked_add_signed(delta)
                .expect("sorted selections");
            match self.typing_at(&selection, ch, closer) {
                Typing::Plain => {
                    edits.push((range.clone(), typed.to_owned()));
                    selections.push(caret(start + len));
                    delta += len as isize - range.len() as isize;
                }
                Typing::Open(close) => {
                    edits.push((range, format!("{ch}{close}")));
                    selections.push(caret(start + len));
                    opened.push((
                        start..start + len,
                        start + len..start + len + close.len_utf8(),
                        (ch, close),
                    ));
                    delta += (len + close.len_utf8()) as isize;
                }
                Typing::Wrap(close) => {
                    edits.push((range.start..range.start, typed.to_owned()));
                    edits.push((range.end..range.end, close.to_string()));
                    let end = start + len + range.len();
                    selections.push(ResolvedSelection {
                        range: start + len..end,
                        reversed: selection.reversed,
                    });
                    opened.push((start..start + len, end..end + close.len_utf8(), (ch, close)));
                    delta += (len + close.len_utf8()) as isize;
                }
                Typing::TypeOver(ix) => {
                    selections.push(caret(start + len));
                    typed_over.push(ix);
                }
            }
        }
        // Typed over, a closer is done with.
        typed_over.sort_unstable();
        typed_over.dedup();
        for ix in typed_over.into_iter().rev() {
            self.auto_closed.remove(ix);
        }
        let snapshot = self.apply_selecting(edits, selections, Typed::insert(typed));
        for (opener, closer, chars) in opened {
            self.auto_closed
                .push(AutoClosed::new(&snapshot, opener, closer, chars));
        }
        self.forget_auto_closed();
    }

    /// What typing `ch` does at `selection`; `closer` is `ch`'s, if it's an
    /// opener.
    fn typing_at(&self, selection: &ResolvedSelection, ch: char, closer: Option<char>) -> Typing {
        if !selection.range.is_empty() {
            return closer.map_or(Typing::Plain, Typing::Wrap);
        }
        let offset = selection.range.start;
        if let Some(ix) = self.auto_closed_before(offset)
            && self.auto_closed[ix].chars.1 == ch
        {
            return Typing::TypeOver(ix);
        }
        let Some(close) = closer else {
            return Typing::Plain;
        };
        let text = self.text();
        let row = self.row_span(offset);
        let next = chars_forward(text, offset..row.end)
            .next()
            .map(|(_, ch)| ch);
        let closes_before =
            next.is_none_or(|next| next.is_whitespace() || CLOSES_BEFORE.contains(&next));
        let prev = chars_backward(text, row.start..offset)
            .next()
            .map(|(_, ch)| ch);
        let quote_ok = close != ch
            || prev.is_none_or(|prev| CharClass::of(prev) != CharClass::Word && prev != ch);
        if closes_before && quote_ok {
            Typing::Open(close)
        } else {
            Typing::Plain
        }
    }

    /// The opener and closer, as one range, of an empty pair typing
    /// inserted, with `offset` between them.
    pub(crate) fn empty_pair_at(&self, offset: usize) -> Option<Range<usize>> {
        let pair = &self.auto_closed[self.auto_closed_before(offset)?];
        (pair.opener.end == offset).then_some(pair.opener.start..pair.closer.end)
    }

    /// The index of the tracked pair whose closer starts at `offset`, with
    /// the pairs as of the snapshot, as [`EditorModel::forget_auto_closed`]
    /// leaves them; callers sync first.
    fn auto_closed_before(&self, offset: usize) -> Option<usize> {
        self.auto_closed
            .binary_search_by_key(&offset, |pair| pair.closer.start)
            .ok()
    }

    /// Forgets the pairs an edit touched since they were made, or with no
    /// caret's head between their halves, and brings the rest up to the
    /// snapshot, sorted by closer. Runs whenever the text is caught up
    /// with or the selections change, so a caret that leaves a pair
    /// forgets it even if it comes straight back.
    pub(crate) fn forget_auto_closed(&mut self) {
        if self.auto_closed.is_empty() {
            return;
        }
        let heads: Vec<usize> = self
            .selections()
            .iter()
            .map(ResolvedSelection::head)
            .collect();
        let snapshot = &self.snapshot;
        let inside = |pair: &AutoClosed| {
            let first = heads.partition_point(|&head| head < pair.opener.end);
            heads
                .get(first)
                .is_some_and(|&head| head <= pair.closer.start)
        };
        self.auto_closed
            .retain_mut(|pair| match pair.advanced(snapshot) {
                Some(advanced) if inside(&advanced) => {
                    *pair = advanced;
                    true
                }
                _ => false,
            });
        self.auto_closed
            .sort_unstable_by_key(|pair| pair.closer.start);
    }
}

#[cfg(test)]
#[path = "pairs_tests.rs"]
mod tests;
