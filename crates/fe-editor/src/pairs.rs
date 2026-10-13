//! Closing brackets and quotes as they're typed: typing an opener inserts
//! its closer too, or wraps a selection in the pair; typing a closer that
//! typing inserted moves over it; and backspace in an empty pair typing
//! inserted deletes both halves.

use std::ops::Range;

use fe_buffer::{Anchor, Bias, BufferSnapshot, ResolvedSelection};

use crate::{CharClass, EditorModel, Typed, caret, chars_backward, chars_forward};

/// The most pairs tracked at once; the oldest are forgotten first.
const MAX_TRACKED: usize = 64;

/// What a closer may come before for typing its opener to close it,
/// besides whitespace and the row's end.
const CLOSES_BEFORE: &[char] = &[';', ':', '.', ',', '=', '}', ']', ')', '>'];

/// A pair typing an opener inserted, anchored to follow edits. Its closer
/// can be typed over while both halves are intact and on one row with a
/// caret.
#[derive(Clone, Debug)]
pub(crate) struct AutoClosed {
    opener: Range<Anchor>,
    closer: Range<Anchor>,
    chars: (char, char),
}

impl AutoClosed {
    /// A pair whose opener and closer are at `opener` and `closer` in
    /// `snapshot`. Each half's anchors stay on it as text is inserted at
    /// either side.
    fn new(
        snapshot: &BufferSnapshot,
        opener: Range<usize>,
        closer: Range<usize>,
        chars: (char, char),
    ) -> Self {
        let anchor = |range: Range<usize>| {
            snapshot.anchor_at(range.start, Bias::Right)..snapshot.anchor_at(range.end, Bias::Left)
        };
        Self {
            opener: anchor(opener),
            closer: anchor(closer),
            chars,
        }
    }

    /// Where the opener and closer are in `snapshot`, if both are still
    /// whole, in order, and on one row.
    fn resolve(&self, snapshot: &BufferSnapshot) -> Option<(Range<usize>, Range<usize>)> {
        let opener = snapshot.resolve_range(&self.opener);
        let closer = snapshot.resolve_range(&self.closer);
        let text = snapshot.text();
        let holds = |range: &Range<usize>, ch: char| {
            range.len() == ch.len_utf8()
                && chars_forward(text, range.clone()).next() == Some((range.start, ch))
        };
        let row = |offset| text.offset_to_point(offset).row;
        (holds(&opener, self.chars.0)
            && holds(&closer, self.chars.1)
            && opener.end <= closer.start
            && row(opener.start) == row(closer.start))
        .then_some((opener, closer))
    }

    /// The pair with its anchors rebased onto `snapshot`.
    fn refreshed(&self, snapshot: &BufferSnapshot) -> Self {
        let refresh =
            |range: &Range<Anchor>| snapshot.refresh(&range.start)..snapshot.refresh(&range.end);
        Self {
            opener: refresh(&self.opener),
            closer: refresh(&self.closer),
            chars: self.chars,
        }
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
    /// - A closer right before a caret, that typing inserted and that is
    ///   still whole and on the caret's row with its opener, is moved over
    ///   rather than typed again.
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
                    selections.push(ResolvedSelection {
                        range: start + len..start + len + range.len(),
                        reversed: selection.reversed,
                    });
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
        let excess = self.auto_closed.len().saturating_sub(MAX_TRACKED);
        self.auto_closed.drain(..excess);
    }

    /// What typing `ch` does at `selection`; `closer` is `ch`'s, if it's an
    /// opener.
    fn typing_at(&self, selection: &ResolvedSelection, ch: char, closer: Option<char>) -> Typing {
        if !selection.range.is_empty() {
            return closer.map_or(Typing::Plain, Typing::Wrap);
        }
        let offset = selection.range.start;
        let tracked = self.auto_closed.iter().position(|pair| {
            pair.chars.1 == ch
                && pair
                    .resolve(&self.snapshot)
                    .is_some_and(|(_, closer)| closer.start == offset)
        });
        if let Some(ix) = tracked {
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
        self.auto_closed.iter().find_map(|pair| {
            let (opener, closer) = pair.resolve(&self.snapshot)?;
            (opener.end == offset && closer.start == offset).then_some(opener.start..closer.end)
        })
    }

    /// Forgets the pairs no longer whole, or with no caret on their row,
    /// and rebases the rest onto the snapshot.
    pub(crate) fn forget_auto_closed(&mut self) {
        if self.auto_closed.is_empty() {
            return;
        }
        let text = self.snapshot.text();
        let rows: Vec<u32> = self
            .selections()
            .iter()
            .map(|selection| text.offset_to_point(selection.head()).row)
            .collect();
        let snapshot = &self.snapshot;
        self.auto_closed = std::mem::take(&mut self.auto_closed)
            .into_iter()
            .filter(|pair| {
                pair.resolve(snapshot).is_some_and(|(opener, _)| {
                    rows.contains(&text.offset_to_point(opener.start).row)
                })
            })
            .map(|pair| pair.refreshed(snapshot))
            .collect();
    }
}

#[cfg(test)]
#[path = "pairs_tests.rs"]
mod tests;
