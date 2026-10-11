//! Marks (`:help mark-motions`): `m{a-z}` (`vim.set_mark`) sets one at the
//! cursor, and `'{a-z}` and `` `{a-z} `` go to one as a motion
//! ([`Motion::Mark`]): to its row's first non-blank, linewise, or to the
//! mark itself, exclusive. `''` and `` `` `` go to the jump mark, where the
//! last jump left from (`:help jump-motions`), and `'<`, `'>`, `` `< `` and
//! `` `> `` to the last visual extent's ends.

use std::{collections::BTreeMap, ops::Range};

use fe_buffer::{Anchor, BufferSnapshot, TransactionId};
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Bias;

use crate::{MarkArgs, MarkMotion, Motion, VimState, VimTarget, motion, visual::Pos};

/// What jumping to a mark that isn't set (or whose row was deleted) says.
pub(crate) const NOT_SET: &str = "E20: Mark not set";

/// Where a mark is, kept as nvim keeps one: a byte column on a row. The row
/// follows rows put in or taken out above it, and edits within it leave the
/// column alone; joined to the row above, the mark goes along with its
/// text (or, after [`Marks::cutting`], keeps its column from where the
/// row's start went). Deleting the row (its text and its line break)
/// deletes the mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    /// The row's start, after text put in there, so a row opened above
    /// moves it down; on an empty row, before it, so one opened below
    /// doesn't.
    row: Anchor,
    /// The row's start, before text put in there: off a row's start once the
    /// line break before it is gone.
    start: Anchor,
    /// The mark's own position (on its row, at most at its end), before
    /// text put in there, which a join carries.
    exact: Anchor,
    /// How far past `exact` the column is (past the row's end).
    past: usize,
    end: End,
    column: usize,
    /// Joined to the row above, it keeps its column, counted from where the
    /// row's start went, rather than its text.
    keeps_column: bool,
}

/// What an edit deleting a mark's row takes. fe-vim's own linewise deletes
/// delete their rows' marks themselves ([`Marks::deleting`]); this is for
/// the rest, other origins' edits included.
#[derive(Clone, Debug, PartialEq, Eq)]
enum End {
    /// The next row's start (before text put in there): the row start
    /// reaching it took the row's text and its line break.
    Next(Anchor),
    /// On an empty row, the line breaks on both sides of it, from the start
    /// of the one before to the end of the one after: taking only one of
    /// them joined it to a neighbour.
    Breaks(Anchor, Anchor),
    /// On the last row, after the first: its end and the line break before
    /// it (after text put in there).
    Last { end: Anchor, before: Anchor },
    /// The only row, or an empty row with no line break on one side: an edit
    /// never deletes it.
    Kept,
}

impl Place {
    /// At `offset` in `snapshot`.
    pub(crate) fn at(snapshot: &BufferSnapshot, offset: usize) -> Self {
        let text = snapshot.text();
        let row = text.offset_to_point(offset).row;
        Self::on(snapshot, row, offset - text.row_start(row))
    }

    /// At `column` (which may lie past its end) on `row` of `snapshot`.
    fn on(snapshot: &BufferSnapshot, row: u32, column: usize) -> Self {
        Self::through(snapshot, row, row, column)
    }

    /// [`Place::on`], deleted only with the rows from `row` through `last`.
    fn through(snapshot: &BufferSnapshot, row: u32, last: u32, column: usize) -> Self {
        let text = snapshot.text();
        let start = text.row_start(row);
        let len = text.row_len(row) as usize;
        let last_end = text.row_start(last) + text.row_len(last) as usize;
        let empty = last_end == start;
        let has_next = last + 1 < text.row_count();
        let end = match row {
            0 if !has_next => End::Kept,
            _ if empty && (row == 0 || !has_next) => End::Kept,
            _ if empty => End::Breaks(
                snapshot.anchor_after(start - 1),
                snapshot.anchor_before(start + 1),
            ),
            _ if has_next => End::Next(snapshot.anchor_before(last_end + 1)),
            _ => End::Last {
                end: snapshot.anchor_before(last_end),
                before: snapshot.anchor_after(start - 1),
            },
        };
        let exact = text.clip_offset(start + column.min(len), Bias::Left);
        let exact_column = exact - start;
        Self {
            row: match empty {
                true => snapshot.anchor_before(start),
                false => snapshot.anchor_after(start),
            },
            start: snapshot.anchor_before(start),
            exact: snapshot.anchor_before(exact),
            past: column - exact_column,
            end,
            column,
            keeps_column: false,
        }
    }

    /// Its row and byte column in `snapshot`, as nvim's `getpos()` reports
    /// them (the column may lie past the row's end), or `None` once its row
    /// was deleted. From a snapshot it can't be resolved in (older than it,
    /// or compacted past it), it's `None` too.
    pub(crate) fn position(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        // `row` may be newer than the rest (see `Marks::typing`).
        let (newest, oldest) = (self.row.version(), self.start.version());
        if newest > snapshot.version() || oldest.min(newest) < snapshot.oldest_version() {
            return None;
        }
        let at = |anchor: &Anchor| snapshot.resolve(anchor);
        let row = at(&self.row);
        let deleted = match &self.end {
            End::Next(next) => row >= at(next),
            End::Breaks(before, after) => at(before) >= at(after),
            End::Last { end, before } => row >= at(end) && row <= at(before),
            End::Kept => false,
        };
        if deleted {
            return None;
        }
        let text = snapshot.text();
        let point = |offset| {
            let point = text.offset_to_point(offset);
            (point.row, point.column as usize)
        };
        Some(match point(at(&self.start)) {
            (_, 0) => (point(row).0, self.column),
            // Joined to the row above.
            (row, start) if self.keeps_column => (row, start + self.column),
            _ => {
                let (row, column) = point(at(&self.exact));
                (row, column + self.past)
            }
        })
    }
}

/// The marks one editor's vim state holds: `a` to `z`, and the jump mark.
#[derive(Clone, Debug, Default)]
pub(crate) struct Marks {
    named: BTreeMap<char, Place>,
    /// `''` and `` `` ``: where the last jump left from.
    jump: Option<Place>,
    /// While a command that jumped runs, the jump mark before it, which
    /// [`settle`] may give back.
    previous: Option<Place>,
}

impl Marks {
    /// Sets the jump mark, keeping the one before until the command
    /// [`settle`]s.
    pub(crate) fn jumped(&mut self, from: Place) {
        self.previous = self.jump.replace(from);
    }

    /// Before a linewise change over the rows from `first` through `last`,
    /// which keeps the first and deletes the rest, as nvim does: their marks
    /// go, and those on the first stay through the change's taking its line
    /// break.
    pub(crate) fn changing(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        let change = |place: &Place| match place.position(snapshot)? {
            // Typing replaces its text: anchored before it.
            (row, column) if row == first => Some(Place {
                row: snapshot.anchor_before(snapshot.text().row_start(row)),
                ..Place::through(snapshot, row, last, column)
            }),
            (row, _) if row > first && row <= last => None,
            _ => Some(place.clone()),
        };
        self.update(change);
    }

    /// Before rows go in above `row` (`O`, a linewise `P`): marks on it, if
    /// it's empty, move down with it, as on any other row.
    pub(crate) fn opening(&mut self, snapshot: &BufferSnapshot, row: u32) {
        let start = snapshot.text().row_start(row);
        self.update(|place| match place.position(snapshot)? {
            (at, column) if at == row => Some(Place {
                row: snapshot.anchor_after(start),
                ..Place::on(snapshot, row, column)
            }),
            _ => Some(place.clone()),
        });
    }

    /// Before text goes in at each of `offsets` (typing, `:s`) that's at a
    /// row's start: as in nvim, it goes before the row's marks, so a line
    /// break in it doesn't move them down.
    pub(crate) fn typing(
        &mut self,
        snapshot: &BufferSnapshot,
        offsets: impl IntoIterator<Item = usize>,
    ) {
        let text = snapshot.text();
        let rows: Vec<u32> = offsets
            .into_iter()
            .map(|offset| text.offset_to_point(offset))
            .filter(|point| point.column == 0)
            .map(|point| point.row)
            .collect();
        if rows.is_empty() {
            return;
        }
        self.update(|place| match place.position(snapshot)? {
            (row, _) if rows.contains(&row) => Some(Place {
                row: snapshot.anchor_before(text.row_start(row)),
                ..place.clone()
            }),
            _ => Some(place.clone()),
        });
    }

    /// Before a linewise delete of the rows from `first` through `last`:
    /// their marks go, as nvim's do.
    pub(crate) fn deleting(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        self.update(|place| match place.position(snapshot)? {
            (row, _) if (first..=last).contains(&row) => None,
            _ => Some(place.clone()),
        });
    }

    /// Before a charwise delete of `range` over rows, which joins what's left
    /// of its last row to its first: as in nvim, the first row's marks stay
    /// even if all its text goes, and the last row's keep their columns,
    /// counted from where the row's start goes, rather than their text.
    pub(crate) fn cutting(&mut self, snapshot: &BufferSnapshot, range: Range<usize>) {
        let text = snapshot.text();
        let (first, last) = (
            text.offset_to_point(range.start).row,
            text.offset_to_point(range.end).row,
        );
        if first == last {
            return;
        }
        self.update(|place| match place.position(snapshot)? {
            (row, column) if row == first => Some(Place::through(snapshot, row, last, column)),
            // Its row is joined, never deleted.
            (row, column) if row == last => Some(Place {
                keeps_column: true,
                end: End::Kept,
                ..Place::on(snapshot, row, column)
            }),
            _ => Some(place.clone()),
        });
    }

    /// Catches the marks up with `snapshot`: those whose rows were deleted
    /// go, and the rest are anchored afresh where they are. Each command
    /// that may change the text calls it first, so a mark on the last row
    /// that rows were put in after is deleted with its row as any other.
    pub(crate) fn follow(&mut self, snapshot: &BufferSnapshot) {
        let follow = |place: &Place| {
            let (row, column) = place.position(snapshot)?;
            Some(Place::on(snapshot, row, column))
        };
        self.update(follow);
    }

    /// Each mark becomes what `f` makes of it, or goes.
    fn update(&mut self, f: impl Fn(&Place) -> Option<Place>) {
        let named = std::mem::take(&mut self.named);
        self.named = named
            .into_iter()
            .filter_map(|(name, place)| Some((name, f(&place)?)))
            .collect();
        self.jump = self.jump.as_ref().and_then(&f);
        self.previous = self.previous.as_ref().and_then(f);
    }
}

/// A mark, as [`VimState::mark`] returns it: `'<` or `'>`, one `m` set
/// (`a` to `z`), or the jump mark (`'` or `` ` ``).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark(Of);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Of {
    /// `'<` or `'>`: the start or end of the last visual extent. Like
    /// nvim's, it's whichever end of it comes first or last when it's read,
    /// as edits may have moved them past each other.
    Visual {
        ends: [Pos; 2],
        end: bool,
        linewise: bool,
    },
    Set(Place),
}

impl Mark {
    /// Its row and byte column in `snapshot` (the editor's, or a newer one),
    /// as nvim's `getpos()` reports them, or `None` once its row was deleted
    /// (`'<` and `'>` never are): the column may lie past the row's end, and
    /// a linewise extent's `'<` is at its row's column 0 and its `'>` on its
    /// row's last byte.
    pub fn get(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        let (ends, end, linewise) = match &self.0 {
            Of::Visual {
                ends,
                end,
                linewise,
            } => (ends, *end, *linewise),
            Of::Set(place) => return place.position(snapshot),
        };
        let [a, b] = [&ends[0], &ends[1]].map(|pos| pos.position(snapshot));
        let (row, column) = if end { a.max(b) } else { a.min(b) };
        let text = snapshot.text();
        let last = |row| match row < text.row_count() {
            true => (text.row_len(row) as usize).saturating_sub(1),
            false => 0,
        };
        Some(match (linewise, end) {
            (true, false) => (row, 0),
            (true, true) => (row, last(row)),
            (false, _) => (row, column),
        })
    }

    /// [`Mark::get`], or `(0, 0)` once its row was deleted.
    pub fn position(&self, snapshot: &BufferSnapshot) -> (u32, usize) {
        self.get(snapshot).unwrap_or((0, 0))
    }
}

impl VimState {
    /// A mark: `a` to `z` once `m` set it, `'` or `` ` `` (the jump mark)
    /// once a jump or `m'` set it, and `'<` and `'>`, the start and end of
    /// the last visual extent, once a visual mode is left by any path. A
    /// linewise extent's `'<` is on its first row's column 0, and its `'>`
    /// on its last row's last byte.
    pub fn mark(&self, name: char) -> Option<Mark> {
        let place = match name {
            'a'..='z' => self.marks.named.get(&name),
            '\'' | '`' => self.marks.jump.as_ref(),
            '<' | '>' => {
                let (ends, linewise) = self.last_visual.as_ref()?.marks();
                return Some(Mark(Of::Visual {
                    ends,
                    end: name == '>',
                    linewise,
                }));
            }
            _ => None,
        };
        Some(Mark(Of::Set(place?.clone())))
    }
}

/// The vim cursor in a visual mode, else the primary caret, with the model
/// caught up.
fn cursor(target: &mut VimTarget<'_>) -> usize {
    target.model.sync_display();
    target
        .state
        .visual_cursor(target.model)
        .unwrap_or_else(|| target.model.primary_selection().head())
}

/// `vim.set_mark`: `a` to `z`, or the jump mark for `'` or `` ` ``, at the
/// vim cursor in a visual mode (which goes on), else at the primary caret.
pub(crate) fn set(
    target: &mut VimTarget<'_>,
    args: MarkArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let name = args.name;
    if !(name.is_ascii_lowercase() || matches!(name, '\'' | '`')) {
        return Err(CommandError::new(format!("m doesn't set mark {name:?}")));
    }
    let at = cursor(target);
    let place = Place::at(target.model.snapshot(), at);
    let marks = &mut target.state.marks;
    match name {
        'a'..='z' => {
            marks.named.insert(name, place);
        }
        _ => marks.jump = Some(place),
    }
    Ok(Dispatched::new(Effect::Nothing))
}

/// `motion` resolved, if it's a mark's: [`Motion::MarkAt`] the mark, its
/// column at most its row's length, or E20 if it isn't set. Others come
/// back as they are.
pub(crate) fn resolve(target: &mut VimTarget<'_>, motion: Motion) -> Result<Motion, String> {
    let Motion::Mark(MarkMotion { name, exact }) = motion else {
        return Ok(motion);
    };
    target.model.sync_display();
    let snapshot = target.model.snapshot();
    let mark = target.state.mark(name).and_then(|mark| mark.get(snapshot));
    let (row, column) = mark.ok_or_else(|| NOT_SET.to_string())?;
    let text = snapshot.text();
    // `'<` and `'>` may stay past the end, on rows a change took.
    let row = row.min(text.row_count() - 1);
    let offset = text.row_start(row) + column.min(text.row_len(row) as usize);
    Ok(Motion::MarkAt {
        offset: text.clip_offset(offset, Bias::Left),
        exact,
    })
}

/// Sets the jump mark at the vim cursor or primary caret, for a command that
/// jumps from there (`:d`, `:s`).
pub(crate) fn jump_here(target: &mut VimTarget<'_>) {
    let at = cursor(target);
    let from = Place::at(target.model.snapshot(), at);
    target.state.marks.jumped(from);
}

/// Ends a command that may have jumped, as nvim does: if the jump left the
/// cursor where the jump mark is, or the command deleted the jump mark's
/// row, the jump mark goes back to the one before (if there was one).
pub(crate) fn settle(target: &mut VimTarget<'_>) {
    let Some(previous) = target.state.marks.previous.take() else {
        return;
    };
    let at = cursor(target);
    let snapshot = target.model.snapshot();
    let point = snapshot.text().offset_to_point(at);
    let jump = target.state.marks.jump.as_ref();
    let back = match jump.and_then(|place| place.position(snapshot)) {
        Some(jump) => jump == (point.row, point.column as usize),
        None => true,
    };
    // As nvim's, one deleted meanwhile isn't given back.
    if back && previous.position(snapshot).is_some() {
        target.state.marks.jump = Some(previous);
    }
}

/// The jump mark a jump by `motion` (resolved, with `count`) from the vim
/// cursor or primary caret sets, if it is one that gets anywhere: `%`
/// finding a bracket, or a count of 100 or less (a search is checked before
/// it runs, and a mark resolved).
pub(crate) fn jump(
    target: &mut VimTarget<'_>,
    motion: Motion,
    count: Option<u32>,
) -> Option<Place> {
    if !motion.is_jump() {
        return None;
    }
    let from = cursor(target);
    let text = target.model.text();
    let lands = match (motion, count) {
        (Motion::MatchPair, Some(count)) => count <= 100,
        (Motion::MatchPair, None) => {
            motion::match_pair(text, motion::Cursor::at(text, from).offset()).is_some()
        }
        _ => true,
    };
    lands.then(|| Place::at(target.model.snapshot(), from))
}
