//! A block session's other rows, followed by who edited them.
//!
//! `Esc` types a session's text on each of a block's other rows as they were
//! when it opened ([`BlockRow::found`]). The session's own typing on one of
//! them, which the cursor keys allow (see [`VimState::split_session`]), doesn't
//! cost it the text, as in nvim: `Esc` then types at the block's display
//! column in the row as it is now. Another origin's edit on it, its line
//! break taken out or text typed across it still do.

use std::ops::Range;

use fe_buffer::{BufferEvent, Edit, EditKind, Origin, Subscription};
use fe_text::Text;

use super::{BlockRow, BlockRows, Edge, point, row_end, visual};

/// How the session's own edits left a row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Fate {
    /// Nobody edited it.
    Same,
    /// Only the session's own typing within it.
    Typed,
    /// Another origin's edit, its line break taken out, or a line break typed
    /// in it.
    Lost,
}

/// A block's other rows through every edit since its session opened: where
/// each is (its text, without the line break ending it) and its [`Fate`].
pub(super) struct Watch {
    events: Subscription,
    rows: Vec<(Range<usize>, Fate)>,
    /// The text as of the last batch followed.
    text: Text,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch")
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl Watch {
    pub(super) fn new(events: Subscription, rows: &BlockRows) -> Self {
        let text = rows.text.clone();
        let rows = rows
            .rows
            .iter()
            .map(|row| (row.text.clone(), Fate::Same))
            .collect();
        Self { events, rows, text }
    }

    pub(super) fn fate(&self, ix: usize) -> Fate {
        self.rows.get(ix).map_or(Fate::Lost, |&(_, fate)| fate)
    }

    pub(super) fn start(&self, ix: usize) -> usize {
        self.rows[ix].0.start
    }

    /// Follows the buffer's edits since the last call: `origin`'s are the
    /// session's (but for its undos and redos).
    pub(super) fn follow(&mut self, origin: &Origin) {
        for event in self.events.drain() {
            let BufferEvent::Edited {
                origin: by,
                kind,
                edits,
                text,
                ..
            } = event
            else {
                continue;
            };
            let own = &by == origin && kind == EditKind::Edit;
            let old = std::mem::replace(&mut self.text, text.clone());
            for (span, fate) in &mut self.rows {
                if *fate == Fate::Lost {
                    continue;
                }
                // Whole rows put in right above it don't touch it.
                let above = |edit: &Edit| {
                    edit.old.is_empty()
                        && edit.old.start == span.start
                        && !edit.new.is_empty()
                        && text.bytes_at(edit.new.end - 1).first() == Some(&b'\n')
                };
                // Whole rows taken out or replaced right above it.
                let removed = |edit: &Edit| {
                    !edit.old.is_empty()
                        && edit.old.end == span.start
                        && (edit.old.start == 0
                            || old.bytes_at(edit.old.start - 1).first() == Some(&b'\n'))
                        && (edit.new.is_empty()
                            || text.bytes_at(edit.new.end - 1).first() == Some(&b'\n'))
                };
                let mut past = false;
                for edit in edits.iter() {
                    if edit.old.start > span.end || edit.old.end < span.start {
                        continue;
                    }
                    if above(edit) {
                        past = true;
                        continue;
                    }
                    if removed(edit) {
                        continue;
                    }
                    let inside = edit.old.start >= span.start && edit.old.end <= span.end;
                    let broken = text.slice_to_string(edit.new.clone()).contains('\n');
                    *fate = match (own && inside && !broken, *fate) {
                        (true, Fate::Same) => Fate::Typed,
                        (true, fate) => fate,
                        _ => Fate::Lost,
                    };
                }
                span.start = point(&edits, span.start, past);
                span.end = point(&edits, span.end, true);
            }
        }
    }
}

/// Where `Esc` types on `row`, whose [`Watch`] found only the session's own
/// typing on it, in `text` (its start `start`): at display `column` (the
/// block's, or `typed`'s) as it is now, padding a row short of it if it was
/// before. The same as [`BlockRows::place`] for the row as it was.
pub(super) fn retyped(
    rows: &BlockRows,
    row: &BlockRow,
    (text, start): (&Text, usize),
    column: Option<usize>,
) -> Option<(Range<usize>, (usize, usize))> {
    if column.is_none() && matches!(rows.edge, Edge::End(_)) {
        let end = row_end(text, text.offset_to_point(start).row) - start;
        return Some((end..end, row.spaces));
    }
    let was = rows.text.slice_to_string(row.text.clone());
    let at = |ix: usize| visual::width_from(&was[..was.floor_char_boundary(ix)], 0, rows.tab);
    let column = column.unwrap_or_else(|| at(row.place.start) + row.spaces.1);
    let pad = row.place.start >= was.len() && at(was.len()) < column;
    let ix = text.offset_to_point(start).row;
    match visual::insertion(text, ix, (Some(column), false), rows.tab, pad) {
        Some((range, spaces, at)) => Some((range.start - start..range.end - start, (spaces, at))),
        None => matches!(rows.edge, Edge::End(_)).then(|| {
            let end = row_end(text, ix) - start;
            (end..end, (0, 0))
        }),
    }
}
