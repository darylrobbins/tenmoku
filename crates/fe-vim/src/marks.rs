//! Marks (`:help mark-motions`): `m{a-z}` (`vim.set_mark`) sets one at the
//! cursor, and `'{a-z}` and `` `{a-z} `` go to one as a motion
//! ([`Motion::Mark`]): to its row's first non-blank, linewise, or to the
//! mark itself, exclusive. `''` and `` `` `` go to the jump mark, where the
//! last jump left from (`:help jump-motions`), and `'<`, `'>`, `` `< `` and
//! `` `> `` to the last visual extent's ends. Each jump that sets the jump
//! mark also adds where it left from to the jumplist, which
//! [`crate::jumplist`]'s keys go back and forward through.
//!
//! `m{A-Z}` sets an uppercase mark, kept and jumped to as `a` to `z` are
//! (in this view: a mark in another file is the host's). `'[` and `']`
//! (`` `[ `` and `` `] ``) go to the first and last char of the text last
//! changed, put or yanked (`:help '[`), and `'.` to where the last change
//! was (`:help '.`): the changes and puts come from the edit batches the
//! marks follow, those of vim's own commands ([`Changes`]), so a yank
//! (which edits nothing) sets `'[` and `']` itself, as does leaving insert
//! mode, which also sets `'^`, where it was left (`:help '^`). `m[` and `m]`
//! set `'[` and `']`.

use std::{
    collections::BTreeMap,
    fmt,
    ops::Range,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_buffer::{
    Anchor, BufferEvent, BufferSnapshot, Edit, EditKind, Origin, Subscription, Text, TransactionId,
    Version,
};
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Bias;

use crate::{
    MarkArgs, MarkMotion, Motion, VimState, VimTarget, motion, prose, unmatched, visual::Pos,
};
use fe_editor::EditorModel;

/// What jumping to a mark that isn't set (or whose row was deleted) says.
pub(crate) const NOT_SET: &str = "E20: Mark not set";

/// What going to a jumplist entry past the last row says.
pub(crate) const INVALID_ROW: &str = "E19: Mark has invalid line number";

/// How many entries the jumplist keeps, as nvim's: the oldest go. The
/// changelist keeps as many.
const JUMPS: usize = 100;

/// What going older than the changelist's oldest entry says.
const AT_START: &str = "E662: At start of changelist";

/// What going newer than the changelist's newest entry says.
const AT_END: &str = "E663: At end of changelist";

/// What going through the changelist with no change made says.
const NO_CHANGES: &str = "E664: Changelist is empty";

/// How many columns from the changelist's newest entry a change on its row
/// may be and still take its place rather than add one: nvim's, with
/// 'textwidth' 0.
const NEAR: usize = 79;

/// Whether `m{ch}` sets a mark: `a` to `z`, `A` to `Z`, `'` and `` ` ``
/// (the jump mark), and `[` and `]`.
pub(crate) fn settable(ch: char) -> bool {
    ch.is_ascii_alphabetic() || "'`[]".contains(ch)
}

/// Whether `'{ch}` and `` `{ch} `` go to a mark: those `m` sets, and `.`,
/// `^`, `<` and `>`.
pub(crate) fn jumpable(ch: char) -> bool {
    settable(ch) || ".^<>".contains(ch)
}

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
    /// Whether its row lives, followed through each edit batch.
    life: Life,
    column: usize,
    /// Joined to the row above, it keeps its column, counted from where the
    /// row's start went, rather than its text.
    keeps_column: bool,
    /// Reflowed (`gq`), it goes with its text onto whichever row that
    /// lands on.
    follows_text: bool,
}

/// One edit batch the buffer applied, as [`Life`] follows it.
#[derive(Clone, Debug)]
struct Batch {
    transaction: TransactionId,
    origin: Origin,
    kind: EditKind,
    from: Version,
    to: Version,
    edits: Arc<[Edit]>,
    /// The text right after it.
    text: Text,
}

/// The buffer's edit batches since the marks last caught up, which a mark's
/// [`Life`] replays: the frozen [`End`] anchors can't tell a row that was
/// emptied from one that was deleted once rows came or went around it.
///
/// Each handle (a clone included) is its own consumer, with its own place in
/// the batches, which are kept until every handle has caught up.
struct Feed {
    state: Arc<Mutex<FeedState>>,
    /// How many batches (counting those dropped) this handle has caught up
    /// with.
    seen: Arc<AtomicUsize>,
}

struct FeedState {
    events: Subscription,
    batches: Vec<Batch>,
    /// How many batches were dropped from the front of `batches`.
    dropped: usize,
    consumers: Vec<Weak<AtomicUsize>>,
    /// What the commands that made the last few batches noted, by the
    /// version each batch made: kept with the batches, so each handle (a
    /// clone's too) reads a batch with its note.
    notes: Vec<(Version, Note)>,
}

impl fmt::Debug for Feed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Feed")
    }
}

impl PartialEq for Feed {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for Feed {}

impl Clone for Feed {
    fn clone(&self) -> Self {
        let mut state = self.lock();
        // Dropped handles are pruned as new ones register, so reading marks
        // in an unchanged buffer (which never clears) doesn't grow this.
        state.consumers.retain(|seen| seen.strong_count() > 0);
        let seen = Arc::new(AtomicUsize::new(self.seen.load(Ordering::Relaxed)));
        state.consumers.push(Arc::downgrade(&seen));
        Self {
            state: self.state.clone(),
            seen,
        }
    }
}

impl FeedState {
    fn receive(&mut self) {
        for event in self.events.drain() {
            if let BufferEvent::Edited {
                transaction,
                origin,
                kind,
                from,
                to,
                edits,
                text,
            } = event
            {
                self.batches.push(Batch {
                    transaction,
                    origin,
                    kind,
                    from,
                    to,
                    edits,
                    text,
                });
            }
        }
    }
}

impl Feed {
    fn new(events: Subscription) -> Self {
        let seen = Arc::new(AtomicUsize::new(0));
        let state = FeedState {
            events,
            batches: Vec::new(),
            dropped: 0,
            consumers: vec![Arc::downgrade(&seen)],
            notes: Vec::new(),
        };
        Self {
            state: Arc::new(Mutex::new(state)),
            seen,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FeedState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The batches this handle hasn't caught up with, with those received
    /// since.
    fn batches(&self) -> Vec<Batch> {
        let mut state = self.lock();
        state.receive();
        let from = self.seen.load(Ordering::Relaxed).max(state.dropped) - state.dropped;
        state.batches[from..].to_vec()
    }

    /// What the commands that made the last few batches noted.
    fn notes(&self) -> Vec<(Version, Note)> {
        self.lock().notes.clone()
    }

    /// Notes `note` about the batch that made `version`.
    fn note(&self, version: Version, note: Note) {
        let notes = &mut self.lock().notes;
        if notes.len() == NOTES {
            notes.remove(0);
        }
        notes.push((version, note));
    }

    /// This handle has caught up with everything received so far.
    fn clear(&self) {
        let mut state = self.lock();
        state.receive();
        self.seen
            .store(state.dropped + state.batches.len(), Ordering::Relaxed);
        state.consumers.retain(|seen| seen.strong_count() > 0);
        let least = state
            .consumers
            .iter()
            .filter_map(Weak::upgrade)
            .map(|seen| seen.load(Ordering::Relaxed))
            .min()
            .unwrap_or(state.dropped);
        let gone = least.saturating_sub(state.dropped);
        state.batches.drain(..gone);
        state.dropped += gone;
    }
}

/// Where a mark's row is, and whether it still is, as of `version`: what
/// each edit batch did to it, from the text it left.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Life {
    version: Version,
    /// The row's start and end offsets.
    start: usize,
    end: usize,
    /// Text put in at the row's start goes before it, as the mark's `row`
    /// anchor's bias says (see [`Marks::typing`] and [`Marks::opening`]);
    /// an empty row's does, unless opened.
    before: bool,
    /// It was empty when the mark was set: only an edit taking the line
    /// breaks on both sides of it deletes it, where one taking a break and
    /// the text of one that wasn't (even if emptied since) does.
    hollow: bool,
    /// Joined by a cut: not deleted by it.
    kept: bool,
    dead: bool,
}

impl Life {
    fn new(snapshot: &BufferSnapshot, start: usize, end: usize) -> Self {
        Self {
            version: snapshot.version(),
            start,
            end,
            before: start == end,
            hollow: start == end,
            kept: false,
            dead: false,
        }
    }

    /// `row` of `text`, the text as of `version`.
    fn on_row(version: Version, text: &Text, row: u32) -> Self {
        let start = text.row_start(row);
        let end = start + text.row_len(row) as usize;
        Self {
            version,
            start,
            end,
            before: start == end,
            hollow: start == end,
            kept: false,
            dead: false,
        }
    }

    /// Applies `batches`, those from its version on. One that doesn't follow
    /// on from it (events missed) leaves it behind, to fall back on anchors.
    fn advance(&mut self, batches: &[Batch]) {
        self.advance_through(batches, None);
    }

    /// [`Life::advance`], but not past `limit`: batches that end after it
    /// are left, so a snapshot's answer doesn't change with later edits.
    fn advance_through(&mut self, batches: &[Batch], limit: Option<Version>) {
        for batch in batches {
            if batch.to <= self.version {
                continue;
            }
            if limit.is_some_and(|limit| batch.to > limit) {
                return;
            }
            if batch.from != self.version {
                return;
            }
            self.step(batch);
            self.version = batch.to;
        }
    }

    /// The row is deleted when an edit takes its text and a line break
    /// next to it (an empty row: both breaks); the only row never is.
    fn step(&mut self, batch: &Batch) {
        if self.dead {
            return;
        }
        let (mut removed, mut added) = (0, 0);
        let mut taken: Vec<Range<usize>> = Vec::new();
        for edit in batch.edits.iter() {
            removed += edit.old.len();
            added += edit.new.len();
            match taken.last_mut() {
                Some(last) if edit.old.start <= last.end => last.end = last.end.max(edit.old.end),
                _ if edit.old.is_empty() => {}
                _ => taken.push(edit.old.clone()),
            }
        }
        let len = batch.text.len() + removed - added;
        let covers = |from: usize, to: usize| {
            taken
                .iter()
                .any(|range| range.start <= from && range.end >= to)
        };
        let (start, end) = (self.start, self.end);
        let before = start > 0 && start <= len;
        let after = end < len;
        let deleted = match self.hollow {
            true => before && after && covers(start - 1, end + 1),
            false => (before && covers(start - 1, end)) || (after && covers(start, end + 1)),
        };
        if deleted && !self.kept {
            self.dead = true;
            return;
        }
        // The join protects against this batch only; the row then lives as
        // any other.
        let joined = std::mem::take(&mut self.kept);
        let at = map(start, self.before, &batch.edits).min(batch.text.len());
        let row = batch.text.offset_to_point(at).row;
        self.start = batch.text.row_start(row);
        self.end = self.start + batch.text.row_len(row) as usize;
        if joined {
            self.hollow = self.start == self.end;
        }
    }
}

/// `offset` after `edits`, staying before (`left`) or going after text
/// inserted at it, as fe-buffer's anchors map: at the start of a replaced
/// range it stays, at its end it goes to the end of the new text, and where
/// a replacement ends at an edit's start, that later edit decides.
fn map(offset: usize, left: bool, edits: &[Edit]) -> usize {
    let mut ix = edits.partition_point(|edit| edit.old.end < offset);
    while ix + 1 < edits.len()
        && edits[ix].old.start < offset
        && edits[ix].old.end == offset
        && edits[ix + 1].old.start == offset
    {
        ix += 1;
    }
    let Some(edit) = edits.get(ix) else {
        return match edits.last() {
            Some(last) => offset + last.new.end - last.old.end,
            None => offset,
        };
    };
    if offset < edit.old.start {
        return offset + edit.new.start - edit.old.start;
    }
    let left = match () {
        _ if edit.old.is_empty() => left,
        _ if offset == edit.old.start => true,
        _ if offset == edit.old.end => false,
        _ => left,
    };
    match left {
        true => edit.new.start,
        false => edit.new.end,
    }
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
            life: Life::new(snapshot, start, last_end),
            column,
            keeps_column: false,
            follows_text: false,
        }
    }

    /// Typing at its row's start goes before it.
    fn before_text(self, snapshot: &BufferSnapshot, start: usize) -> Self {
        Self {
            row: snapshot.anchor_before(start),
            life: Life {
                before: true,
                ..self.life
            },
            ..self
        }
    }

    /// Caught up with `batches`.
    fn advance(&mut self, batches: &[Batch]) {
        self.life.advance(batches);
    }

    /// Caught up with `batches` through those that end by `version`.
    fn advance_through(&mut self, batches: &[Batch], version: Version) {
        self.life.advance_through(batches, Some(version));
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
        // Followed through each batch up to this snapshot, its life knows;
        // else the anchors do, as of when the mark was set.
        let followed = self.life.version <= snapshot.version();
        let deleted = match (followed, self.life.version == snapshot.version()) {
            (true, true) => self.life.dead,
            _ => {
                (followed && self.life.dead)
                    || match &self.end {
                        End::Next(next) => row >= at(next),
                        End::Breaks(before, after) => at(before) >= at(after),
                        End::Last { end, before } => row >= at(end) && row <= at(before),
                        End::Kept => false,
                    }
            }
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
            _ if self.follows_text => {
                let (row, column) = point(at(&self.exact));
                (row, column + self.past)
            }
            (_, 0) => (point(row).0, self.column),
            // Joined to the row above.
            (row, start) if self.keeps_column => (row, start + self.column),
            _ => {
                let (row, column) = point(at(&self.exact));
                (row, column + self.past)
            }
        })
    }

    /// Where it is as a jumplist entry, which a deleted row doesn't delete,
    /// as nvim keeps one: [`Place::position`], or once its row was deleted,
    /// the row that took its place at the same column. `None` past the last
    /// row, where no row took it (or where it can't be resolved).
    fn spot(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        if let Some(at) = self.position(snapshot) {
            return Some(at);
        }
        let (newest, oldest) = (self.row.version(), self.start.version());
        if newest > snapshot.version() || oldest.min(newest) < snapshot.oldest_version() {
            return None;
        }
        // Its row's start went where the deletion was: the start of the row
        // after it, or the end of the one before when that was the last.
        let text = snapshot.text();
        let point = text.offset_to_point(snapshot.resolve(&self.row));
        let row = match point.column {
            0 => point.row,
            _ => point.row + 1,
        };
        (row < text.row_count()).then_some((row, self.column))
    }

    /// Whether it was set or followed past `snapshot`'s version.
    fn newer_than(&self, snapshot: &BufferSnapshot) -> bool {
        self.row.version() > snapshot.version() || self.life.version > snapshot.version()
    }

    /// Joined to the row above by a cut, keeping its column from where its
    /// row's start goes.
    fn joined(snapshot: &BufferSnapshot, row: u32, column: usize) -> Self {
        let mut place = Place::on(snapshot, row, column);
        place.keeps_column = true;
        place.end = End::Kept;
        place.life.kept = true;
        place
    }
}

/// A jumplist entry: where a jump left from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    /// Followed as a mark is, though a deleted row doesn't delete it (see
    /// [`Place::spot`]).
    At(Place),
    /// Past the last row, where the rows it was on and those after went: no
    /// row ever takes it again.
    Past,
}

impl Entry {
    fn spot(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        match self {
            Entry::At(place) => place.spot(snapshot),
            Entry::Past => None,
        }
    }
}

/// `'[`, `']` and `'.` (`:help '[`, `:help '.`): set from each edit batch of
/// vim's own (one whose transaction fe-vim noted carets for, or typed in its
/// insert session), as nvim's commands set them, and followed through the
/// others as marks are. A yank, a shift that changed nothing and leaving
/// insert mode set `'[` and `']` themselves ([`yanked`], [`shifted`],
/// [`left_insert`]).
///
/// A batch's edits say where the text changed but not which command
/// changed it, so it's read by its shape, with what the command noted
/// ([`Note`]): whole rows taken out (`dd`) leave `'[` and `']` on the row
/// after them, at the column the operator began at, and `'.` at its start;
/// whole rows put in (a linewise `p`) span `'[` (column 0) to `']` (their
/// last row's last byte); rows a shift noted span its column on the first
/// to the last's last byte; line breaks joined away (`J`) span the first
/// join to the joined row's end; anything else spans its first char to its
/// last, both at the start of a deletion, with `'.` at its start (or, past
/// a line break it took out, the start of the row after). Text typed on
/// from where the last typing ended, with no line break, leaves `'.` where
/// that began, as nvim's typed-ahead keys go in at once. An undo or a redo
/// spans the rows its batches changed, column 0. A row past the last (`dd`
/// there) leaves the mark there, which a jump can't go to (E19).
///
/// Each of vim's batches that sets `'.` also makes it the changelist's
/// newest entry (`:help changelist`), as nvim's `changed_common` does: until
/// a transaction (nvim's undoable change) has added an entry, its batches
/// add one unless the newest is on the same row within [`NEAR`] columns, in
/// which case they take its place, as every later batch of the transaction
/// does (an undo or a redo starts no new transaction). Entries follow edits
/// as jumplist entries do: a deleted row doesn't delete one.
///
/// The batches wait in `pending` until a command with the vim state at hand
/// reads them ([`catch_up`]), which knows whose they are.
#[derive(Clone, Debug, Default)]
struct Changes {
    /// The batches the marks took since the last [`catch_up`].
    pending: Vec<Batch>,
    /// The text before the next batch, and its version: what a batch took
    /// out is measured in it.
    text: Option<(Version, Text)>,
    /// `'[`.
    start: Option<Spot>,
    /// `']`.
    end: Option<Spot>,
    /// `'.`.
    change: Option<Spot>,
    /// Where the open insert session began, for `'[` once it's left.
    began: Option<Spot>,
    /// While an insert session is open, the version it opened at and its
    /// origin: that origin's batches from it on are typing.
    typing: Option<(Version, Origin)>,
    /// Where the last text typed with no line break ended, in the text
    /// after it.
    run: Option<usize>,
    /// The last of vim's batches read: an undo's or a redo's next batch
    /// spans the rows of both.
    last: Option<(TransactionId, EditKind)>,
    /// The transactions of the last few insert sessions, vim's own before
    /// their carets record is kept (once they end).
    owned: Vec<TransactionId>,
    /// While an insert session this state left to another state ending it
    /// ([`Marks::closing`]) is yet to be adopted, which marks were set
    /// since: those are newer than what it left, which the batches ending
    /// it ([`Note::Ending`]) don't set and adopting it doesn't either.
    closing: Option<Newer>,
    /// The changelist, oldest first, at most [`JUMPS`] entries: each one
    /// followed as a [`Spot`], placed as an [`Entry`] is ([`Spot::entry`]).
    list: Vec<Spot>,
    /// The entry `g;` and `g,` last went to, or the list's length when
    /// neither has since the last change.
    at: usize,
    /// The transaction last read has added no entry yet: its next batch
    /// setting `'.` may (nvim's `b_new_change`).
    fresh: bool,
}

/// Which of the marks an insert session sets were set since it was left to
/// another state ending it, kept apart as nvim's commands set them apart: a
/// yank sets only `'[` and `']`, `m[` and `m]` one of them, a change `'.`
/// too, and only another session `'^`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct Newer {
    start: bool,
    end: bool,
    change: bool,
    exit: bool,
}

impl Newer {
    /// `'[` and `']`.
    const ENDS: Self = Self {
        start: true,
        end: true,
        change: false,
        exit: false,
    };
    const CHANGE: Self = Self {
        change: true,
        ..Self::ENDS
    };
    const ALL: Self = Self {
        exit: true,
        ..Self::CHANGE
    };

    fn or(self, other: Self) -> Self {
        Self {
            start: self.start || other.start,
            end: self.end || other.end,
            change: self.change || other.change,
            exit: self.exit || other.exit,
        }
    }
}

/// What a command noted about the batch it made, which the batch can't say
/// ([`Marks::note`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// Whole rows deleted, the operator having begun at this column, which
    /// `'[` takes, as nvim's.
    Deleted { column: usize },
    /// A characterwise delete: rows it takes whole, line breaks and all,
    /// are text taken out like any other, `'.` going to the row after.
    Chars,
    /// A shift of the rows from the first to the last, begun at `column`:
    /// `'[` and `']` span them, whether or not it changed them all.
    Shifted { column: usize, rows: (u32, u32) },
    /// A case operator's change: `'[` and `']` span what it covered, from
    /// `start` to `end` (row and byte column, in the text after), whether or
    /// not it changed all of it.
    Recased {
        start: (u32, usize),
        end: (u32, usize),
    },
    /// A batch that changed nothing (the hidden undo step of a case operator
    /// that left the text as it was): it sets no mark.
    Unchanged,
    /// Whole rows put in, from the first to the last (in the text after),
    /// which an empty row put next to an empty row can't say.
    Put { rows: (u32, u32) },
    /// Rows joined (`J`): `'[` at the first join, `']` at the end of the
    /// row they're joined onto.
    Joined,
    /// Insert mode left with this batch the session's last: `'.` at
    /// `change` (row and byte column), where the count's copies or a
    /// block's rows left it, whatever the batch's shape says.
    Left { change: (u32, usize) },
    /// The state ending an insert session made it (the count's copies, the
    /// indentation `Esc` took out, a block's other rows): it's older than
    /// what a clone of that state, which left the session to it, did since.
    Ending,
}

/// How many insert sessions' transactions [`Changes`] keeps.
const SESSIONS: usize = 16;

/// How many batches' notes a [`Feed`] keeps: those the commands of every
/// handle (each clone's) made, a few for each, so a handle reading the
/// batches only some commands later still finds them.
const NOTES: usize = 64;

impl PartialEq for Changes {
    fn eq(&self, other: &Self) -> bool {
        let version = |changes: &Self| changes.text.as_ref().map(|(version, _)| *version);
        let pending = |changes: &Self| {
            let batches = changes.pending.iter();
            batches.map(|batch| batch.to).collect::<Vec<_>>()
        };
        version(self) == version(other)
            && pending(self) == pending(other)
            && self.start == other.start
            && self.end == other.end
            && self.change == other.change
            && self.began == other.began
            && self.typing == other.typing
            && self.run == other.run
            && self.last == other.last
            && self.owned == other.owned
            && self.closing == other.closing
            && self.list == other.list
            && self.at == other.at
            && self.fresh == other.fresh
    }
}

impl Eq for Changes {}

/// One of [`Changes`]' marks: a row, and its column there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Spot {
    row: Row,
    column: Column,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    /// Set from a batch, which has no snapshot to anchor it in: its row is
    /// followed through each batch since, until [`Marks::follow`] places it.
    Followed {
        life: Life,
        column: usize,
    },
    Placed(Place),
    /// Past the last row: no row ever takes it.
    Past,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Column {
    /// The row's own.
    Own,
    /// The row's last byte, as a linewise yank's `']` is.
    End,
}

impl Spot {
    /// At `own` on `row` of `text`, the text as of `version`; past the last
    /// row if it hasn't that row.
    fn followed(version: Version, text: &Text, (row, own): (u32, usize)) -> Self {
        Self {
            row: match row < text.row_count() {
                true => Row::Followed {
                    life: Life::on_row(version, text, row),
                    column: own,
                },
                false => Row::Past,
            },
            column: Column::Own,
        }
    }

    /// At `offset` in `snapshot`.
    fn placed_at(snapshot: &BufferSnapshot, offset: usize) -> Self {
        Self::placed(Place::at(snapshot, offset))
    }

    fn placed(place: Place) -> Self {
        Self {
            row: Row::Placed(place),
            column: Column::Own,
        }
    }

    fn advance(&mut self, batch: &Batch) {
        match &mut self.row {
            Row::Followed { life, .. } => life.advance(std::slice::from_ref(batch)),
            Row::Placed(place) => place.advance(std::slice::from_ref(batch)),
            Row::Past => {}
        }
    }

    /// Its row in `text`, the text right after the batch it was last
    /// advanced through, if it's followed there.
    fn row_in(&self, text: &Text, version: Version) -> Option<u32> {
        match &self.row {
            Row::Followed { life, .. } if life.version == version && !life.dead => {
                Some(text.offset_to_point(life.start).row)
            }
            _ => None,
        }
    }

    /// Its row and byte column in `snapshot`, as [`Place::position`]
    /// reports them.
    fn position(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        let (row, own) = match &self.row {
            Row::Followed { life, column } => {
                if life.dead || life.version != snapshot.version() {
                    return None;
                }
                (snapshot.text().offset_to_point(life.start).row, *column)
            }
            Row::Placed(place) => place.position(snapshot)?,
            Row::Past => return None,
        };
        let column = match self.column {
            Column::Own => own,
            Column::End => (snapshot.text().row_len(row) as usize).saturating_sub(1),
        };
        Some((row, column))
    }

    /// As a jumplist entry: placed, or past the last row.
    fn of_entry(entry: Entry) -> Self {
        match entry {
            Entry::At(place) => Self::placed(place),
            Entry::Past => Self {
                row: Row::Past,
                column: Column::Own,
            },
        }
    }

    /// Where it is as a changelist entry, which a deleted row doesn't
    /// delete: placed, as [`Place::spot`] says; followed, as
    /// [`Spot::position`] does (one whose row went with the batches it was
    /// followed through is past the last row).
    fn entry(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        match &self.row {
            Row::Placed(place) => place.spot(snapshot),
            _ => self.position(snapshot),
        }
    }

    /// Its row and byte column in `text`, the text right after the batch it
    /// was last advanced through, at `version`, if it's followed there.
    fn point_in(&self, text: &Text, version: Version) -> Option<(u32, usize)> {
        let (life, column) = match &self.row {
            Row::Followed { life, column } => (life, *column),
            Row::Placed(place) => (&place.life, place.column),
            Row::Past => return None,
        };
        (life.version == version && !life.dead)
            .then(|| (text.offset_to_point(life.start).row, column))
    }

    /// Placed afresh in `snapshot` as a changelist entry: where
    /// [`Spot::entry`] says, else past the last row; as it is if it's newer
    /// than `snapshot` (or, followed, not as new).
    fn place_entry(&self, snapshot: &BufferSnapshot) -> Self {
        let newer = match &self.row {
            Row::Followed { life, .. } => life.version != snapshot.version(),
            Row::Placed(place) => place.newer_than(snapshot),
            Row::Past => false,
        };
        if newer {
            return self.clone();
        }
        Self::of_entry(match self.entry(snapshot) {
            Some((row, column)) => Entry::At(Place::on(snapshot, row, column)),
            None => Entry::Past,
        })
    }

    /// Anchored in `snapshot` where it is, `None` once its row is gone, or
    /// as it is if it's newer than `snapshot`.
    fn place(&self, snapshot: &BufferSnapshot) -> Option<Self> {
        let Row::Followed { life, .. } = &self.row else {
            return Some(self.clone());
        };
        if life.dead {
            return None;
        }
        if life.version != snapshot.version() {
            return Some(self.clone());
        }
        let (row, column) = self.position(snapshot)?;
        let column = match self.column {
            Column::Own => column,
            Column::End => 0,
        };
        Some(Self {
            row: Row::Placed(Place::on(snapshot, row, column)),
            column: self.column,
        })
    }
}

impl Changes {
    fn spots(&mut self) -> impl Iterator<Item = &mut Option<Spot>> {
        [
            &mut self.start,
            &mut self.end,
            &mut self.change,
            &mut self.began,
        ]
        .into_iter()
    }

    fn is_set(&self) -> bool {
        self.start.is_some()
            || self.end.is_some()
            || self.change.is_some()
            || self.began.is_some()
            || !self.list.is_empty()
    }

    fn mark(&self, name: char) -> Option<&Spot> {
        match name {
            '[' => self.start.as_ref(),
            ']' => self.end.as_ref(),
            _ => self.change.as_ref(),
        }
    }

    /// Reads the pending batches up to those that end by `limit`, `own`
    /// saying which are vim's (besides those typed in its insert session),
    /// `caret` the column of a transaction's first caret before it, `notes`
    /// what their commands noted.
    fn commit(
        &mut self,
        limit: Option<Version>,
        own: &dyn Fn(TransactionId) -> bool,
        caret: &dyn Fn(TransactionId) -> Option<usize>,
        notes: &[(Version, Note)],
    ) {
        let batches = std::mem::take(&mut self.pending);
        let read = match limit {
            Some(limit) => batches.partition_point(|batch| batch.to <= limit),
            None => batches.len(),
        };
        for batch in &batches[..read] {
            let before = match &self.text {
                Some((version, _)) if batch.to <= *version => continue,
                Some((version, text)) if *version == batch.from => Some(text.clone()),
                _ => None,
            };
            for spot in self.spots().flatten() {
                spot.advance(batch);
            }
            for entry in &mut self.list {
                entry.advance(batch);
            }
            let typed = self
                .typing
                .as_ref()
                .is_some_and(|(from, origin)| batch.from >= *from && batch.origin == *origin);
            let owned = self.owned.contains(&batch.transaction);
            let noted: Vec<&Note> = notes
                .iter()
                .filter(|(at, _)| *at == batch.to)
                .map(|(_, note)| note)
                .collect();
            match typed || owned || own(batch.transaction) {
                true => self.read(batch, before.as_ref(), typed, caret, &noted),
                false => self.run = None,
            }
            self.text = Some((batch.to, batch.text.clone()));
        }
        self.pending = batches[read..].to_vec();
    }

    /// Sets the marks from vim's own `batch`, the text before it `before`
    /// if known (see [`Changes`]), with what its command noted.
    fn read(
        &mut self,
        batch: &Batch,
        before: Option<&Text>,
        typed: bool,
        caret: &dyn Fn(TransactionId) -> Option<usize>,
        noted: &[&Note],
    ) {
        // The state ending a session this one left doesn't set the marks set
        // since; anything else is newer than it.
        let kept = match &mut self.closing {
            Some(newer) if noted.contains(&&Note::Ending) => Some((
                *newer,
                [&self.start, &self.end, &self.change].map(Clone::clone),
            )),
            Some(newer) => {
                *newer = Newer::CHANGE.or(*newer);
                None
            }
            None => None,
        };
        let changed = self.set(batch, before, typed, caret, noted);
        // `'.` set since the session was left stays, and so does the
        // changelist.
        if changed && !kept.as_ref().is_some_and(|(newer, _)| newer.change) {
            self.record(batch);
        }
        if let Some((newer, [start, end, change])) = kept {
            let marks = [&mut self.start, &mut self.end, &mut self.change];
            for ((mark, kept), newer) in marks.into_iter().zip([start, end, change]).zip([
                newer.start,
                newer.end,
                newer.change,
            ]) {
                if newer {
                    *mark = kept;
                }
            }
        }
    }

    /// [`Changes::read`], for whichever state's command made `batch`:
    /// whether it set `'.`.
    fn set(
        &mut self,
        batch: &Batch,
        before: Option<&Text>,
        typed: bool,
        caret: &dyn Fn(TransactionId) -> Option<usize>,
        noted: &[&Note],
    ) -> bool {
        let (Some(first), Some(last)) = (batch.edits.first(), batch.edits.last()) else {
            return false;
        };
        let note = noted.iter().find(|note| ***note != Note::Ending);
        if note.is_some_and(|note| **note == Note::Unchanged) {
            return false;
        }
        let (text, version) = (&batch.text, batch.to);
        let point = |offset: usize| {
            let point = text.offset_to_point(offset);
            (point.row, point.column as usize)
        };
        let at = |offset: usize| Some(Spot::followed(version, text, point(offset)));
        let on = |row: u32, column: usize| Some(Spot::followed(version, text, (row, column)));
        let last_byte = |row: u32| match row < text.row_count() {
            true => (text.row_len(row) as usize).saturating_sub(1),
            false => 0,
        };
        let run = self.run.take();
        let previous = self.last.replace((batch.transaction, batch.kind));
        // A new undoable change, as nvim's undo block: an undo or a redo
        // starts none.
        if batch.kind == EditKind::Edit && previous.is_none_or(|(id, _)| id != batch.transaction) {
            self.fresh = true;
        }
        if batch.kind != EditKind::Edit {
            let mut top = point(first.new.start).0;
            let mut bottom = match last.new.is_empty() {
                true => point(last.new.start).0,
                false => point(last.new.end - 1).0,
            };
            // The same undo's (or redo's) batches span all their rows.
            if previous == self.last {
                let row = |spot: &Option<Spot>| spot.as_ref()?.row_in(text, version);
                top = row(&self.start).map_or(top, |row| row.min(top));
                bottom = row(&self.end).map_or(bottom, |row| row.max(bottom));
            }
            (self.start, self.end, self.change) = (on(top, 0), on(bottom, 0), on(top, 0));
            return true;
        }
        let note = note.map(|note| (*note).clone());
        let one_line = |range: &Range<usize>| point(range.start).0 == point(range.end).0;
        // Typing on with no line break: `'.` stays where it began.
        let types_on =
            typed && batch.edits.len() == 1 && first.old.is_empty() && one_line(&first.new);
        if types_on && !first.new.is_empty() {
            self.run = Some(first.new.end);
        }
        let keeps_change = types_on && run == Some(first.old.start);
        let column = || match note {
            Some(Note::Deleted { column }) => column,
            _ => caret(batch.transaction).unwrap_or(0),
        };
        let deletes = batch.edits.iter().all(|edit| edit.new.is_empty());
        let put = match note {
            Some(Note::Put { rows }) => Some(rows),
            _ => put_rows(text, first).filter(|_| !typed),
        };
        let (start, end, change) = if let Some(Note::Recased { start, end }) = note {
            (on(start.0, start.1), on(end.0, end.1), at(first.new.start))
        } else if let Some(Note::Shifted {
            column,
            rows: (row, last_row),
        }) = note
        {
            (
                on(row, column),
                on(last_row, last_byte(last_row)),
                on(row, 0),
            )
        } else if let Some(Note::Joined) = note {
            let row = point(first.new.start).0;
            let end = text.row_len(row) as usize;
            (at(first.new.start), on(row, end), on(row + 1, 0))
        } else if let Some(row) = before.filter(|_| deletes).and_then(|before| {
            // All the text, rows only as the operator said.
            let all = first.old == (0..before.len());
            match (&note, all) {
                (Some(Note::Deleted { .. }), true) => Some(0),
                (Some(Note::Chars), _) => None,
                (_, true) => None,
                _ => taken_rows(before, &first.old),
            }
        }) {
            (on(row, column()), on(row, column()), on(row, 0))
        } else if let Some((row, last_row)) = put {
            (on(row, 0), on(last_row, last_byte(last_row)), on(row, 0))
        } else {
            let end = match last.new.is_empty() {
                true => last.new.start,
                false => text.clip_offset(last.new.end - 1, Bias::Left),
            };
            // A line break taken out leaves `'.` on the row after.
            let broke = before.is_some_and(|before| {
                let row = |offset| before.offset_to_point(offset).row;
                row(first.old.start) != row(first.old.end)
            });
            let change = match broke {
                true => on(point(first.new.start).0 + 1, 0),
                false => at(first.new.start),
            };
            (at(first.new.start), at(end), change)
        };
        self.start = start;
        self.end = end;
        if !keeps_change {
            self.change = change;
        }
        if let Some(Note::Left {
            change: (row, column),
        }) = note
        {
            self.change = on(row, column);
            return true;
        }
        !keeps_change
    }

    /// `'.`, just set from `batch`, made the changelist's newest entry,
    /// added or in the newest's place (see [`Changes`]).
    fn record(&mut self, batch: &Batch) {
        let Some(change) = self.change.clone() else {
            return;
        };
        let (text, version) = (&batch.text, batch.to);
        let add = match self.list.last() {
            None => true,
            Some(_) if !self.fresh => false,
            Some(newest) => match (
                newest.point_in(text, version),
                change.point_in(text, version),
            ) {
                (Some((row, column)), Some((at, own))) => {
                    row != at || column + NEAR < own || own + NEAR < column
                }
                _ => true,
            },
        };
        match self.list.last_mut() {
            Some(newest) if !add => *newest = change,
            _ => {
                self.fresh = false;
                if self.list.len() == JUMPS {
                    self.list.remove(0);
                }
                self.list.push(change);
            }
        }
        self.at = self.list.len();
    }

    /// `marks` were set since a session was left to the state ending it.
    fn newer(&mut self, marks: Newer) {
        if let Some(newer) = &mut self.closing {
            *newer = marks.or(*newer);
        }
    }

    /// Anchored afresh where they are in `snapshot`, or gone with their
    /// rows; one newer than `snapshot` (another model edited the buffer
    /// since) stays as it is.
    fn follow(&mut self, snapshot: &BufferSnapshot) {
        for spot in self.spots() {
            *spot = match spot.take() {
                Some(Spot {
                    row: Row::Placed(place),
                    column,
                }) if !place.newer_than(snapshot) => {
                    place.position(snapshot).map(|(row, at)| Spot {
                        row: Row::Placed(Place::on(snapshot, row, at)),
                        column,
                    })
                }
                spot => spot.and_then(|spot| spot.place(snapshot)),
            };
        }
        for entry in &mut self.list {
            *entry = entry.place_entry(snapshot);
        }
    }

    /// Each placed mark becomes what `f` makes of it, or goes; a
    /// changelist entry `f` would delete stays as it is.
    fn update(&mut self, f: impl Fn(&Place) -> Option<Place>) {
        for spot in self.spots() {
            if let Some(Spot {
                row: Row::Placed(place),
                column,
            }) = spot
            {
                *spot = f(place).map(|place| Spot {
                    row: Row::Placed(place),
                    column: *column,
                });
            }
        }
        for entry in &mut self.list {
            if let Row::Placed(place) = &mut entry.row
                && let Some(moved) = f(place)
            {
                *place = moved;
            }
        }
    }

    /// Each placed changelist entry becomes what `f` makes of where it is
    /// as a jumplist entry ([`Spot::entry`]), if anything: what moves
    /// jumplist entries ([`Marks::displace`], [`Marks::cutting`]) moves
    /// these too. One followed has yet to be placed, after the edit.
    fn move_entries(
        &mut self,
        snapshot: &BufferSnapshot,
        f: impl Fn((u32, usize)) -> Option<Entry>,
    ) {
        for entry in &mut self.list {
            if let Row::Placed(place) = &entry.row
                && let Some(moved) = place.spot(snapshot).and_then(&f)
            {
                *entry = Spot::of_entry(moved);
            }
        }
    }
}

/// The first of the whole rows `range` of `text` takes out, if it takes
/// only whole rows, each with its line break (the last rows, with the line
/// break before them).
fn taken_rows(text: &Text, range: &Range<usize>) -> Option<u32> {
    if range.is_empty() {
        return None;
    }
    let (start, end) = (
        text.offset_to_point(range.start),
        text.offset_to_point(range.end),
    );
    match () {
        _ if start.column == 0 && end.column == 0 => Some(start.row),
        _ if range.end == text.len() && start.column == text.row_len(start.row) => {
            Some(start.row + 1)
        }
        _ => None,
    }
}

/// The first and last of the whole rows `edit` put in `text`, if it put
/// only whole rows, each with its line break (after the last row, with the
/// line break before them).
fn put_rows(text: &Text, edit: &Edit) -> Option<(u32, u32)> {
    if !edit.old.is_empty() || edit.new.is_empty() {
        return None;
    }
    let (start, end) = (
        text.offset_to_point(edit.new.start),
        text.offset_to_point(edit.new.end),
    );
    match () {
        _ if start.column == 0 && end.column == 0 => Some((start.row, end.row - 1)),
        _ if edit.new.end == text.len() && start.column == text.row_len(start.row) => {
            Some((start.row + 1, end.row))
        }
        _ => None,
    }
}

/// The marks one editor's vim state holds: `a` to `z`, `'^`, `A` to `Z`,
/// the jump mark, and `'[`, `']` and `'.` ([`Changes`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct Marks {
    /// `a` to `z`, and `^`.
    named: BTreeMap<char, Place>,
    /// `A` to `Z`: as nvim's, a deleted row doesn't delete one, which goes
    /// to the row that took its place, as a jumplist entry does.
    files: BTreeMap<char, Entry>,
    changes: Changes,
    /// `''` and `` `` ``: where the last jump left from.
    jump: Option<Place>,
    /// While a command that jumped runs, the jump mark before it, which
    /// [`settle`] may give back.
    previous: Option<Place>,
    /// The buffer's edit batches, from the first mark set on.
    feed: Option<Feed>,
    /// The jumplist (`:help jumplist`), oldest first: where each jump that
    /// set the jump mark left from, at most [`JUMPS`] of them. Like nvim's,
    /// an earlier entry on a later one's row stays until the list is used
    /// ([`Marks::travel`]).
    list: Vec<Entry>,
    /// The entry `<C-o>` and `<C-i>` last went to, or the list's length when
    /// neither has since the last jump.
    at: usize,
    /// While a `:g` runs its commands, their jumps add nothing (`:help
    /// jumplist`).
    keeping: bool,
    /// The first and last rows the first `cx` marked
    /// (`feature/exchange.rs`), each at its start: they follow edits as `a`
    /// to `z` do, each gone once its row is deleted.
    pub(crate) exchange: Option<[Option<Place>; 2]>,
}

impl Marks {
    /// Starts following `model`'s buffer, before a mark is set.
    pub(crate) fn watch(&mut self, model: &mut EditorModel) {
        if self.feed.is_none() {
            let events = model.buffer_mut().subscribe();
            self.feed = Some(Feed::new(events));
            let snapshot = model.snapshot();
            self.changes.text = Some((snapshot.version(), snapshot.text().clone()));
        }
    }

    /// An insert session of `origin`'s opened at `at` in `snapshot`: what
    /// it types from there on leaves `'.` where typing began, until a line
    /// break, and leaving insert mode leaves `'[` at `at`.
    pub(crate) fn inserting(&mut self, snapshot: &BufferSnapshot, at: usize, origin: Origin) {
        self.changes.newer(Newer::ALL);
        self.changes.typing = Some((snapshot.version(), origin));
        self.changes.run = None;
        // Typing at its row's start goes after it, as nvim's `'[` stays.
        let start = snapshot
            .text()
            .row_start(snapshot.text().offset_to_point(at).row);
        let began = Place::at(snapshot, at).before_text(snapshot, start);
        self.changes.began = Some(Spot::placed(began));
    }

    /// What the command whose batch made `version` noted about it, which
    /// the batch can't say.
    pub(crate) fn note(&mut self, version: Version, note: Note) {
        if let Some(feed) = &self.feed {
            feed.note(version, note);
        }
    }

    /// The open insert session is left to another state ending it (a clone
    /// of this one's, which shares its session): its outcome is adopted
    /// once that publishes it ([`Marks::adopting`]), unless the marks were
    /// set since.
    pub(crate) fn closing(&mut self) {
        self.changes.closing = Some(Newer::default());
    }

    /// Transaction `id` is an insert session's: its batches are vim's own,
    /// before its carets record is kept.
    pub(crate) fn own(&mut self, id: TransactionId) {
        let owned = &mut self.changes.owned;
        if owned.len() == SESSIONS {
            owned.remove(0);
        }
        owned.push(id);
    }

    /// The batches the marks haven't caught up with.
    fn batches(&self) -> Vec<Batch> {
        self.feed.as_ref().map(Feed::batches).unwrap_or_default()
    }

    /// Catches each mark's row up with the batches since.
    fn advance(&mut self) {
        let batches = self.batches();
        if batches.is_empty() {
            return;
        }
        self.changes.pending.extend(batches.iter().cloned());
        let entries = self.list.iter_mut().chain(self.files.values_mut());
        let entries = entries.filter_map(|entry| match entry {
            Entry::At(place) => Some(place),
            Entry::Past => None,
        });
        let exchange = self.exchange.iter_mut().flatten().flatten();
        let places = self
            .named
            .values_mut()
            .chain(self.jump.iter_mut())
            .chain(self.previous.iter_mut())
            .chain(exchange)
            .chain(entries);
        for place in places {
            place.advance(&batches);
        }
        if let Some(feed) = &self.feed {
            feed.clear();
        }
    }

    /// Sets the jump mark, keeping the one before until the command
    /// [`settle`]s, and adds `from` to the jumplist.
    pub(crate) fn jumped(&mut self, from: Place) {
        self.add(from.clone());
        self.previous = self.jump.replace(from);
    }

    /// Adds `from` to the end of the jumplist, past which `<C-o>` starts
    /// again.
    fn add(&mut self, from: Place) {
        if self.keeping {
            return;
        }
        self.list.push(Entry::At(from));
        if self.list.len() > JUMPS {
            self.list.remove(0);
        }
        self.at = self.list.len();
    }

    /// Sets the jump mark to `place` as a command's last word on it, adding
    /// nothing to the jumplist: [`settle`] leaves it.
    pub(crate) fn set_jump(&mut self, place: Place) {
        self.previous = Some(place.clone());
        self.jump = Some(place);
    }

    /// Adds where a `:g` began to the jumplist, before it runs its commands
    /// (whose own jumps then add nothing, [`Marks::keep_jumps`]), so the
    /// entry follows what they do.
    pub(crate) fn add_jump(&mut self, from: Place) {
        self.add(from);
    }

    /// While `keep`, jumps add nothing to the jumplist, as while nvim's
    /// `:g` runs its commands.
    pub(crate) fn keep_jumps(&mut self, keep: bool) {
        self.keeping = keep;
    }

    /// Catches the list up with the batches and drops each entry that a
    /// later one is on the row of (adjusting the position), as nvim's
    /// `cleanup_jumplist` does before the list is used or shown.
    fn tidy(&mut self, snapshot: &BufferSnapshot) {
        self.advance();
        // Entries past the last row are all on the row after it, as nvim's
        // line numbers are.
        let past = snapshot.text().row_count();
        let rows: Vec<u32> = self
            .list
            .iter()
            .map(|entry| entry.spot(snapshot).map_or(past, |(row, _)| row))
            .collect();
        let mut at = self.at;
        let mut kept = Vec::with_capacity(self.list.len());
        for (ix, entry) in std::mem::take(&mut self.list).into_iter().enumerate() {
            let later = rows[ix + 1..].contains(&rows[ix]);
            match later {
                true if ix < self.at => at -= 1,
                true => {}
                false => kept.push(entry),
            }
        }
        self.list = kept;
        self.at = at;
    }

    /// The jumplist as `:jumps` lists it: each entry's row and byte column
    /// (a row past the last for one that went), oldest first, and the
    /// position `<C-o>` and `<C-i>` last went to (the length if neither has
    /// since the last jump).
    pub(crate) fn jump_entries(&mut self, snapshot: &BufferSnapshot) -> (Vec<(u32, usize)>, usize) {
        self.tidy(snapshot);
        let past = snapshot.text().row_count();
        let list = self
            .list
            .iter()
            .map(|entry| entry.spot(snapshot).unwrap_or((past, 0)))
            .collect();
        (list, self.at)
    }

    /// The changelist as `:changes` lists it, as [`Marks::jump_entries`]
    /// does the jumplist, once the batches are read ([`catch_up`]).
    pub(crate) fn change_entries(&self, snapshot: &BufferSnapshot) -> (Vec<(u32, usize)>, usize) {
        let past = snapshot.text().row_count();
        let list = self
            .changes
            .list
            .iter()
            .map(|spot| spot.entry(snapshot).unwrap_or((past, 0)))
            .collect();
        (list, self.changes.at)
    }

    /// Goes `count` entries back (`back`, `<C-o>`) or forward (`<C-i>`)
    /// through the jumplist from the cursor at `from`, as nvim does. First an
    /// earlier entry on a later one's row goes, and from past the newest
    /// entry, the newest if it's on the cursor's row. Going back from there
    /// adds where it goes from (sets the jump mark there), so `<C-i>` comes
    /// back; while a `:g` runs ([`Marks::keep_jumps`]) it adds nothing, but
    /// counts as if it had. It returns the entry's row and byte column
    /// (which may lie past its row's end), or `Some(None)` for one past the
    /// last row (E19); past either end of the list, `None`, with the jump
    /// mark and cursor as they were (though once the count fits the list as
    /// it was, the entry added and the step onto it stay, as nvim's do).
    pub(crate) fn travel(
        &mut self,
        snapshot: &BufferSnapshot,
        from: usize,
        back: bool,
        count: u32,
    ) -> Option<Option<(u32, usize)>> {
        self.tidy(snapshot);
        // Not gone back yet, the newest entry goes if it's on the cursor's
        // row, even if going then fails.
        let row = snapshot.text().offset_to_point(from).row;
        let newest = self.list.last().and_then(|entry| entry.spot(snapshot));
        if self.at == self.list.len() && newest.is_some_and(|(newest, _)| newest == row) {
            self.list.pop();
            self.at = self.list.len();
        }
        let count = count.max(1) as usize;
        if back && self.at == self.list.len() {
            // A count past the oldest entry changes nothing. Else, as nvim's,
            // it adds where it goes from (nothing while a `:g` runs) and
            // steps onto the newest entry for good, even if the count then
            // reaches past the oldest (the oldest entry gone to make room).
            self.at.checked_sub(count)?;
            let here = (!self.keeping).then(|| Place::at(snapshot, from));
            if let Some(here) = &here {
                self.list.push(Entry::At(here.clone()));
                if self.list.len() > JUMPS {
                    self.list.remove(0);
                }
            }
            self.at = self.list.len() - 1;
            self.at = self.at.checked_sub(count)?;
            if let Some(here) = here {
                self.previous = self.jump.replace(here);
            }
        } else {
            self.at = match back {
                true => self.at.checked_sub(count)?,
                false => self
                    .at
                    .checked_add(count)
                    .filter(|&at| at < self.list.len())?,
            };
        }
        let found = self.list[self.at].spot(snapshot);
        if found.is_none() {
            // It goes nowhere, so nothing gives the jump mark back.
            self.previous = None;
        }
        Some(found)
    }

    /// Goes `count` entries back (`older`, `g;`) or forward (`g,`) through
    /// the changelist, as nvim's `get_changelist` does, once the batches are
    /// read ([`catch_up`]): a count past either end goes to that end, unless
    /// the entry last gone to is already there (E662 or E663; E664 with no
    /// entry), with nothing changed. From past the newest entry (no `g;` or
    /// `g,` since the last change), `g,` goes to the newest. It returns the
    /// entry's row and byte column (which may lie past its row's end), or
    /// `None` for one past the last row (E19).
    pub(crate) fn go_change(
        &mut self,
        snapshot: &BufferSnapshot,
        older: bool,
        count: u32,
    ) -> Result<Option<(u32, usize)>, &'static str> {
        let changes = &mut self.changes;
        let len = changes.list.len();
        if len == 0 {
            return Err(NO_CHANGES);
        }
        let (at, count) = (changes.at, count.max(1) as usize);
        changes.at = match older {
            true if at == 0 => return Err(AT_START),
            true => at.saturating_sub(count),
            false if at == len - 1 => return Err(AT_END),
            false => (at + count).min(len - 1),
        };
        Ok(changes.list[changes.at].entry(snapshot))
    }

    /// Before the rows from `first` through `last` go: their jumplist
    /// entries go to the row after them, which takes their place, at the
    /// same column, as nvim's do; past the last row if none does, or onto
    /// the only row left if every row goes.
    fn displace(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        self.advance();
        let rows = snapshot.text().row_count();
        let moved = |(row, column): (u32, usize)| {
            (first..=last).contains(&row).then(|| match () {
                _ if last + 1 < rows => Entry::At(Place::on(snapshot, last + 1, column)),
                _ if first == 0 => Entry::At(Place::joined(snapshot, 0, column)),
                _ => Entry::Past,
            })
        };
        for entry in self.list.iter_mut().chain(self.files.values_mut()) {
            if let Some(moved) = entry.spot(snapshot).and_then(moved) {
                *entry = moved;
            }
        }
        self.changes.move_entries(snapshot, moved);
    }

    /// Before a linewise change over the rows from `first` through `last`,
    /// which keeps the first and deletes the rest, as nvim does: their marks
    /// go, and those on the first stay through the change's taking its line
    /// break.
    pub(crate) fn changing(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        if last > first {
            self.displace(snapshot, first + 1, last);
        }
        let change = |place: &Place| match place.position(snapshot)? {
            // Typing replaces its text: anchored before it.
            (row, column) if row == first => {
                let start = snapshot.text().row_start(row);
                Some(Place::through(snapshot, row, last, column).before_text(snapshot, start))
            }
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
            (at, column) if at == row => {
                let mut place = Place::on(snapshot, row, column);
                place.row = snapshot.anchor_after(start);
                place.life.before = false;
                Some(place)
            }
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
            (row, _) if rows.contains(&row) => {
                Some(place.clone().before_text(snapshot, text.row_start(row)))
            }
            _ => Some(place.clone()),
        });
    }

    /// Before a linewise delete of the rows from `first` through `last`:
    /// their marks go, as nvim's do.
    pub(crate) fn deleting(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        self.displace(snapshot, first, last);
        self.update(|place| match place.position(snapshot)? {
            (row, _) if (first..=last).contains(&row) => None,
            _ => Some(place.clone()),
        });
    }

    /// Before `gq` reflows the rows from `first` through `last`, joining
    /// and breaking them only at blanks: as in nvim, their marks go with
    /// their text, and their rows are never deleted.
    pub(crate) fn reflowing(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        self.update(|place| match place.position(snapshot)? {
            (row, column) if (first..=last).contains(&row) => {
                let mut place = Place::on(snapshot, row, column);
                place.follows_text = true;
                place.end = End::Kept;
                place.life.kept = true;
                Some(place)
            }
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
        // Jumplist and changelist entries on the rows between go with the
        // last row's text.
        self.advance();
        let moved = |(row, column): (u32, usize)| {
            (row > first && row < last).then(|| Entry::At(Place::joined(snapshot, last, column)))
        };
        for entry in self.list.iter_mut().chain(self.files.values_mut()) {
            if let Some(moved) = entry.spot(snapshot).and_then(moved) {
                *entry = moved;
            }
        }
        self.changes.move_entries(snapshot, moved);
        self.update(|place| match place.position(snapshot)? {
            (row, column) if row == first => Some(Place::through(snapshot, row, last, column)),
            // Its row is joined, never deleted.
            (row, column) if row == last => Some(Place::joined(snapshot, row, column)),
            _ => Some(place.clone()),
        });
    }

    /// Catches the marks up with `snapshot`: those whose rows were deleted
    /// go, and the rest are anchored afresh where they are. Each command
    /// that may change the text calls it first, so a mark on the last row
    /// that rows were put in after is deleted with its row as any other.
    pub(crate) fn follow(&mut self, snapshot: &BufferSnapshot) {
        // A jumplist entry whose row was deleted stays, on the row that took
        // its place.
        self.advance();
        self.changes.follow(snapshot);
        for entry in self.list.iter_mut().chain(self.files.values_mut()) {
            *entry = match entry.spot(snapshot) {
                Some((row, column)) => Entry::At(Place::on(snapshot, row, column)),
                None => Entry::Past,
            };
        }
        let follow = |place: &Place| {
            let (row, column) = place.position(snapshot)?;
            Some(Place::on(snapshot, row, column))
        };
        self.update_but_changes(follow);
        let entries = (self.list.iter().chain(self.files.values()))
            .any(|entry| matches!(entry, Entry::At(_)));
        let changes = self.changes.is_set() || !self.changes.pending.is_empty();
        let set = !self.named.is_empty() || changes;
        if !set && self.jump.is_none() && self.previous.is_none() && !entries {
            self.feed = None;
        }
    }

    /// Each mark becomes what `f` makes of it, or goes; a jumplist entry
    /// `f` would delete stays as it is.
    fn update(&mut self, f: impl Fn(&Place) -> Option<Place>) {
        self.changes.update(&f);
        self.update_but_changes(f);
    }

    /// [`Marks::update`], leaving `'[`, `']` and `'.` as they are.
    fn update_but_changes(&mut self, f: impl Fn(&Place) -> Option<Place>) {
        self.advance();
        let named = std::mem::take(&mut self.named);
        self.named = named
            .into_iter()
            .filter_map(|(name, place)| Some((name, f(&place)?)))
            .collect();
        self.jump = self.jump.as_ref().and_then(&f);
        self.previous = self.previous.as_ref().and_then(&f);
        for place in self.exchange.iter_mut().flatten() {
            *place = place.as_ref().and_then(&f);
        }
        for entry in self.list.iter_mut().chain(self.files.values_mut()) {
            if let Entry::At(place) = entry
                && let Some(moved) = f(place)
            {
                *place = moved;
            }
        }
    }
}

/// A mark, as [`VimState::mark`] returns it: `'<` or `'>`, one `m` set
/// (`a` to `z`, `A` to `Z`), the jump mark (`'` or `` ` ``), `'^`, or one
/// of `'[`, `']` and `'.`.
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
    /// Followed through the batches since it was read, from `Feed`.
    Set(Place, Feed),
    /// `A` to `Z`: [`Of::Set`], but on the row that took its row's place
    /// once that's deleted.
    Kept(Place, Feed),
    /// `A` to `Z` past the last row, where the rows it was on went.
    Past,
    /// `'[`, `']` or `'.` (`name`): the batches pending and since it was
    /// read, from `Feed`, may set it, those of the transactions in `own`
    /// being vim's, with the column of each one's first caret in `carets`.
    Change {
        name: char,
        changes: Box<Changes>,
        feed: Feed,
        own: Vec<TransactionId>,
        carets: Vec<(TransactionId, usize)>,
    },
}

/// Where a [`Mark`] is.
enum Found {
    At((u32, usize)),
    /// Past the last row, where a jump to it fails with E19.
    Past,
    Gone,
}

impl Mark {
    /// Its row and byte column in `snapshot` (the editor's, or a newer one),
    /// as nvim's `getpos()` reports them, or `None` once its row was deleted
    /// (`'<` and `'>` never are): the column may lie past the row's end, and
    /// a linewise extent's `'<` is at its row's column 0 and its `'>` on its
    /// row's last byte.
    pub fn get(&self, snapshot: &BufferSnapshot) -> Option<(u32, usize)> {
        match self.find(snapshot) {
            Found::At(at) => Some(at),
            Found::Past | Found::Gone => None,
        }
    }

    fn find(&self, snapshot: &BufferSnapshot) -> Found {
        let found = |at: Option<(u32, usize)>| at.map_or(Found::Gone, Found::At);
        let (ends, end, linewise) = match &self.0 {
            Of::Visual {
                ends,
                end,
                linewise,
            } => (ends, *end, *linewise),
            Of::Set(place, feed) => {
                let mut place = place.clone();
                place.advance_through(&feed.batches(), snapshot.version());
                return found(place.position(snapshot));
            }
            Of::Kept(place, feed) => {
                let mut place = place.clone();
                place.advance_through(&feed.batches(), snapshot.version());
                return place.spot(snapshot).map_or(Found::Past, Found::At);
            }
            Of::Past => return Found::Past,
            Of::Change {
                name,
                changes,
                feed,
                own,
                carets,
            } => {
                let mut changes = changes.clone();
                changes.pending.extend(feed.batches());
                let caret = |id| carets.iter().find(|(at, _)| *at == id).map(|&(_, at)| at);
                let own = |id| own.contains(&id);
                changes.commit(Some(snapshot.version()), &own, &caret, &feed.notes());
                return match changes.mark(*name) {
                    Some(Spot { row: Row::Past, .. }) => Found::Past,
                    spot => found(spot.and_then(|spot| spot.position(snapshot))),
                };
            }
        };
        let [a, b] = [&ends[0], &ends[1]].map(|pos| pos.position(snapshot));
        let (row, column) = if end { a.max(b) } else { a.min(b) };
        let text = snapshot.text();
        let last = |row| match row < text.row_count() {
            true => (text.row_len(row) as usize).saturating_sub(1),
            false => 0,
        };
        Found::At(match (linewise, end) {
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
    /// How many feed consumers are registered, for tests.
    #[cfg(test)]
    pub(crate) fn marks_consumers(&self) -> usize {
        self.marks
            .feed
            .as_ref()
            .map_or(0, |feed| feed.lock().consumers.len())
    }

    /// A mark: `a` to `z` and `A` to `Z` once `m` set it, `'` or `` ` ``
    /// (the jump mark) once a jump or `m'` set it, `'<` and `'>`, the start
    /// and end of the last visual extent, once a visual mode is left by any
    /// path, `'^` once insert mode is, and `'[`, `']` and `'.` once a change
    /// (or for `'[` and `']`, a yank or `m`) set them. A linewise extent's
    /// `'<` is on its first row's column 0, and its `'>` on its last row's
    /// last byte, as is a linewise yank's or put's `']`.
    pub fn mark(&self, name: char) -> Option<Mark> {
        let place = match name {
            '[' | ']' | '.' => {
                let feed = self.marks.feed.as_ref()?.clone();
                let changes = &self.marks.changes;
                let batches = feed.batches();
                let ids = changes.pending.iter().chain(&batches);
                let mut ids: Vec<TransactionId> = ids.map(|batch| batch.transaction).collect();
                ids.dedup();
                let own = ids
                    .iter()
                    .copied()
                    .filter(|&id| self.carets.get(id).is_some());
                let carets = ids
                    .iter()
                    .filter_map(|&id| Some((id, self.carets.get(id)?.first()?.1)))
                    .collect();
                return Some(Mark(Of::Change {
                    name,
                    changes: Box::new(changes.clone()),
                    feed,
                    own: own.collect(),
                    carets,
                }));
            }
            'A'..='Z' => {
                let Entry::At(place) = self.marks.files.get(&name)? else {
                    return Some(Mark(Of::Past));
                };
                let feed = self.marks.feed.as_ref()?.clone();
                return Some(Mark(Of::Kept(place.clone(), feed)));
            }
            'a'..='z' | '^' => self.marks.named.get(&name),
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
        // Not caught up here: each read replays from this baseline only as
        // far as its snapshot, which a later one's lifetime would pass.
        let place = place?.clone();
        let feed = self.marks.feed.as_ref()?.clone();
        Some(Mark(Of::Set(place, feed)))
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

/// `vim.set_mark`: `a` to `z`, `A` to `Z`, `'[` or `']`, or the jump mark
/// for `'` or `` ` ``, at the vim cursor in a visual mode (which goes on),
/// else at the primary caret.
pub(crate) fn set(
    target: &mut VimTarget<'_>,
    args: MarkArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let name = args.name;
    if !settable(name) {
        return Err(CommandError::new(format!("m doesn't set mark {name:?}")));
    }
    let at = cursor(target);
    target.state.marks.watch(target.model);
    // The batches before it don't set `'[` or `']` after.
    catch_up(target.state);
    let snapshot = target.model.snapshot();
    let place = Place::at(snapshot, at);
    let marks = &mut target.state.marks;
    match name {
        'a'..='z' => {
            marks.named.insert(name, place);
        }
        'A'..='Z' => {
            marks.files.insert(name, Entry::At(place));
        }
        '[' | ']' => {
            let spot = Some(Spot::placed_at(snapshot, at));
            match name {
                '[' => {
                    marks.changes.newer(Newer {
                        start: true,
                        ..Newer::default()
                    });
                    marks.changes.start = spot;
                }
                _ => {
                    marks.changes.newer(Newer {
                        end: true,
                        ..Newer::default()
                    });
                    marks.changes.end = spot;
                }
            }
        }
        // `m'` adds a jump, as nvim's does.
        _ => {
            marks.add(place.clone());
            marks.jump = Some(place);
        }
    }
    Ok(Dispatched::new(Effect::Nothing))
}

/// `motion` resolved, if it's a mark's: [`Motion::MarkAt`] the mark, its
/// column at most its row's length, or E20 if it isn't set (E19 past the
/// last row). Others come back as they are.
pub(crate) fn resolve(target: &mut VimTarget<'_>, motion: Motion) -> Result<Motion, String> {
    if let Motion::Position { row, column } = motion {
        target.model.sync_display();
        let text = target.model.snapshot().text();
        let row = row.min(text.row_count() - 1);
        let offset = text.row_start(row) + (column as usize).min(text.row_len(row) as usize);
        return Ok(Motion::MarkAt {
            offset: text.clip_offset(offset, Bias::Left),
            exact: true,
        });
    }
    let Motion::Mark(MarkMotion { name, exact }) = motion else {
        return Ok(motion);
    };
    target.model.sync_display();
    let snapshot = target.model.snapshot();
    let found = target.state.mark(name).map(|mark| mark.find(snapshot));
    let (row, column) = match found {
        Some(Found::At(at)) => at,
        Some(Found::Past) => return Err(INVALID_ROW.to_string()),
        _ => return Err(NOT_SET.to_string()),
    };
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
    target.state.marks.watch(target.model);
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
/// finding a bracket, a count of 100 or less, `[( [{ ]) ]}` finding their
/// bracket, or `{ } ( )` not running out
/// of text (a search is checked before it runs, and a mark resolved).
pub(crate) fn jump(
    target: &mut VimTarget<'_>,
    motion: Motion,
    count: Option<u32>,
) -> Option<Place> {
    if !motion.is_jump() {
        return None;
    }
    let from = cursor(target);
    target.state.marks.watch(target.model);
    let text = target.model.text();
    let lands = match (motion, count) {
        (Motion::MatchPair, Some(count)) => count <= 100,
        (Motion::MatchPair, None) => {
            motion::match_pair(text, motion::Cursor::at(text, from).offset()).is_some()
        }
        (Motion::Unmatched { brace, forward }, count) => {
            unmatched::find(text, from, brace, forward, count.unwrap_or(1)).is_some()
        }
        (
            Motion::ParagraphForward
            | Motion::ParagraphBackward
            | Motion::SentenceForward
            | Motion::SentenceBackward,
            count,
        ) => prose::lands(text, from, motion, count),
        _ => true,
    };
    lands.then(|| Place::at(target.model.snapshot(), from))
}

/// `:delmarks`' deletion of the mark `name`: `a` to `z`, `A` to `Z`, `'^`,
/// `'[`, `']`, `'.`, `'<` or `'>`. Others (the jump mark, `0` to `9`) it
/// doesn't delete.
pub(crate) fn delete(state: &mut VimState, name: char) {
    match name {
        'a'..='z' | '^' => {
            state.marks.named.remove(&name);
        }
        'A'..='Z' => {
            state.marks.files.remove(&name);
        }
        '[' | ']' | '.' => {
            // Batches still pending would set them after.
            catch_up(state);
            let changes = &mut state.marks.changes;
            match name {
                '[' => {
                    changes.newer(Newer {
                        start: true,
                        ..Newer::default()
                    });
                    changes.start = None;
                }
                ']' => {
                    changes.newer(Newer {
                        end: true,
                        ..Newer::default()
                    });
                    changes.end = None;
                }
                _ => {
                    changes.newer(Newer {
                        change: true,
                        ..Newer::default()
                    });
                    changes.change = None;
                }
            }
        }
        '<' | '>' => {
            let gone = state.last_visual.as_mut();
            if gone.is_some_and(|visual| visual.delete(name == '>')) {
                state.last_visual = None;
            }
        }
        _ => {}
    }
}

/// `:delmarks!`: every lowercase mark, and `'^`, `'[`, `']` and `'.`.
pub(crate) fn delete_lowercase(state: &mut VimState) {
    for name in ('a'..='z').chain(['^', '[', ']', '.']) {
        delete(state, name);
    }
}

/// `:clearjumps`: the jumplist is emptied (the jump mark stays).
pub(crate) fn clear_jumps(state: &mut VimState) {
    state.marks.list.clear();
    state.marks.at = 0;
}

/// Reads the batches pending for `'[`, `']` and `'.` ([`Changes`]): those
/// of the transactions fe-vim noted carets for are vim's own.
pub(crate) fn catch_up(state: &mut VimState) {
    state.marks.advance();
    let VimState { marks, carets, .. } = state;
    let caret = |id| Some(carets.get(id)?.first()?.1);
    let notes = marks.feed.as_ref().map(Feed::notes).unwrap_or_default();
    marks
        .changes
        .commit(None, &|id| carets.get(id).is_some(), &caret, &notes);
}

/// Catches the marks up with the text before a command that may change it
/// (see [`Marks::follow`]), and follows the buffer from there, so what the
/// command changes sets `'[`, `']` and `'.`.
pub(crate) fn follow(target: &mut VimTarget<'_>) {
    catch_up(target.state);
    target.state.marks.follow(target.model.snapshot());
    target.state.marks.watch(target.model);
    crate::feature::line_undo::follow(target.state, target.model);
}

/// What a yank took, for `'[` and `']`.
pub(crate) enum Yanked {
    /// From `start` to `last`, the offset of its last char (a block's top
    /// left and bottom right corners).
    Chars { start: usize, last: usize },
    /// The rows from the first to the last.
    Rows(u32, u32),
}

/// After a yank of `yanked` in `model`'s text, which edits nothing, as
/// nvim's: `'[` on its first char and `']` on its last, or on the first
/// row's column 0 and the last row's last byte.
pub(crate) fn yanked(state: &mut VimState, model: &mut EditorModel, yanked: Yanked) {
    state.marks.watch(model);
    catch_up(state);
    let snapshot = model.snapshot();
    let (start, end) = match yanked {
        Yanked::Rows(first, last) => (
            Spot::placed(Place::on(snapshot, first, 0)),
            Spot {
                row: Row::Placed(Place::on(snapshot, last, 0)),
                column: Column::End,
            },
        ),
        Yanked::Chars { start, last } => (
            Spot::placed_at(snapshot, start),
            Spot::placed_at(snapshot, last.max(start)),
        ),
    };
    state.marks.changes.newer(Newer::ENDS);
    state.marks.changes.start = Some(start);
    state.marks.changes.end = Some(end);
}

/// A shift of the rows from `first` to `last` of `model`'s text, begun at
/// `column`: its batch, which made `version`, sets `'[` and `']` across
/// those rows (whether or not it changed them all); with no batch, having
/// changed nothing, it sets them and `'.` itself, as nvim's does.
pub(crate) fn shifted(
    state: &mut VimState,
    model: &mut EditorModel,
    version: Option<Version>,
    (first, last): (u32, u32),
    column: usize,
) {
    if let Some(version) = version {
        let rows = (first, last);
        state.marks.note(version, Note::Shifted { column, rows });
        return;
    }
    state.marks.watch(model);
    catch_up(state);
    let snapshot = model.snapshot();
    let last_byte = (snapshot.text().row_len(last) as usize).saturating_sub(1);
    let changes = &mut state.marks.changes;
    changes.newer(Newer::CHANGE);
    changes.start = Some(Spot::placed(Place::on(snapshot, first, column)));
    changes.end = Some(Spot::placed(Place::on(snapshot, last, last_byte)));
    changes.change = Some(Spot::placed(Place::on(snapshot, first, 0)));
}

/// A filter or `:r !` (`ex/filter.rs`) just applied to `model`'s text:
/// `'[` and `'.` at the start of row `first`, `']` at the start of row
/// `last` (each the last row, past the end), as nvim's set them whatever
/// text its batches changed, once those are read.
pub(crate) fn filtered(state: &mut VimState, model: &mut EditorModel, first: u32, last: u32) {
    state.marks.watch(model);
    catch_up(state);
    let snapshot = model.snapshot();
    let rows = snapshot.text().row_count();
    let start = |row: u32| Spot::placed(Place::on(snapshot, row.min(rows - 1), 0));
    let changes = &mut state.marks.changes;
    changes.newer(Newer::CHANGE);
    changes.start = Some(start(first));
    changes.end = Some(start(last));
    changes.change = Some(start(first));
}

/// A case operator covered `start` to `end` (row and byte column) of
/// `model`'s text: its batch, which made `version`, sets `'[` and `']` there
/// (and `'.` as the batch says); with no batch, having changed nothing, it
/// sets `'[` and `']` itself and leaves `'.` where it was, as nvim's does.
pub(crate) fn recased(
    state: &mut VimState,
    model: &mut EditorModel,
    version: Option<Version>,
    start: (u32, usize),
    end: (u32, usize),
) {
    if let Some(version) = version {
        state.marks.note(version, Note::Recased { start, end });
        return;
    }
    state.marks.watch(model);
    catch_up(state);
    let snapshot = model.snapshot();
    let changes = &mut state.marks.changes;
    changes.newer(Newer::ENDS);
    changes.start = Some(Spot::placed(Place::on(snapshot, start.0, start.1)));
    changes.end = Some(Spot::placed(Place::on(snapshot, end.0, end.1)));
}

/// Where leaving insert mode left `'[`, `']` and `'^`, for a clone of the
/// state the session was shared with to adopt.
#[derive(Clone, Debug)]
pub(crate) struct Left {
    start: Place,
    end: Place,
    exit: Place,
    /// `'.`, where the count's copies or a block's rows put it: a clone
    /// may have read their batch before it was noted ([`Note::Left`]).
    change: Option<Place>,
}

/// Insert mode left at `exit` in `model`'s text, what it typed ending at
/// `end` (on a block's last row; else `exit`): `'[` is where the session
/// began, `']` at `end` and `'^` at `exit`, as nvim's, and `'.` at `change`
/// if the count's copies or a block's rows went in last, which is noted
/// with the last batch too, for a clone of the state that reads it.
pub(crate) fn left_insert(
    state: &mut VimState,
    model: &mut EditorModel,
    exit: usize,
    end: usize,
    change: Option<usize>,
) -> Left {
    state.marks.watch(model);
    // With the session's last batch, for every state reading it: a clone's
    // too, which shares the session.
    if let Some(change) = change {
        let snapshot = model.snapshot();
        let point = snapshot.text().offset_to_point(change);
        let change = (point.row, point.column as usize);
        state.marks.note(snapshot.version(), Note::Left { change });
    }
    catch_up(state);
    let snapshot = model.snapshot();
    let changes = &mut state.marks.changes;
    changes.typing = None;
    changes.run = None;
    let began = changes.began.take();
    let start = match began.and_then(|spot| spot.place(snapshot)) {
        Some(Spot {
            row: Row::Placed(place),
            ..
        }) => place,
        _ => Place::at(snapshot, exit),
    };
    let left = Left {
        start,
        end: Place::at(snapshot, end),
        exit: Place::at(snapshot, exit),
        change: change.map(|change| Place::at(snapshot, change)),
    };
    adopt_insert(state, left.clone());
    left
}

/// Takes the marks a clone of this state left insert mode with ([`Left`]),
/// leaving a newer session open here as it is, and those set since it was
/// left to the clone ([`Marks::closing`]).
pub(crate) fn adopt_insert(state: &mut VimState, left: Left) {
    // The batches before it don't set them after.
    catch_up(state);
    let marks = &mut state.marks;
    let newer = marks.changes.closing.take().unwrap_or_default();
    if state.session.is_none() {
        marks.changes.typing = None;
        marks.changes.began = None;
    }
    let changes = &mut marks.changes;
    if !newer.start {
        changes.start = Some(Spot::placed(left.start));
    }
    if !newer.end {
        changes.end = Some(Spot::placed(left.end));
    }
    if let Some(change) = left.change.filter(|_| !newer.change) {
        changes.change = Some(Spot::placed(change));
    }
    if !newer.exit {
        marks.named.insert('^', left.exit);
    }
}

/// A session this state left to a clone ending it ([`Marks::closing`]) was
/// published without marks to adopt (or is newer than its change): the
/// marks go on as they are.
pub(crate) fn closed(state: &mut VimState) {
    state.marks.changes.closing = None;
}
