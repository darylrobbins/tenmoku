//! Marks (`:help mark-motions`): `m{a-z}` (`vim.set_mark`) sets one at the
//! cursor, and `'{a-z}` and `` `{a-z} `` go to one as a motion
//! ([`Motion::Mark`]): to its row's first non-blank, linewise, or to the
//! mark itself, exclusive. `''` and `` `` `` go to the jump mark, where the
//! last jump left from (`:help jump-motions`), and `'<`, `'>`, `` `< `` and
//! `` `> `` to the last visual extent's ends. Each jump that sets the jump
//! mark also adds where it left from to the jumplist, which
//! [`crate::jumplist`]'s keys go back and forward through.

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
    Anchor, BufferEvent, BufferSnapshot, Edit, Subscription, Text, TransactionId, Version,
};
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Bias;

use crate::{MarkArgs, MarkMotion, Motion, VimState, VimTarget, motion, prose, visual::Pos};
use fe_editor::EditorModel;

/// What jumping to a mark that isn't set (or whose row was deleted) says.
pub(crate) const NOT_SET: &str = "E20: Mark not set";

/// What going to a jumplist entry past the last row says.
pub(crate) const INVALID_ROW: &str = "E19: Mark has invalid line number";

/// How many entries the jumplist keeps, as nvim's: the oldest go.
const JUMPS: usize = 100;

/// Whether `m{ch}` sets a mark: `a` to `z`, and `'` and `` ` `` (the jump
/// mark).
pub(crate) fn settable(ch: char) -> bool {
    ch.is_ascii_lowercase() || matches!(ch, '\'' | '`')
}

/// Whether `'{ch}` and `` `{ch} `` go to a mark: `a` to `z`, `'` and `` ` ``
/// (the jump mark), and `<` and `>`.
pub(crate) fn jumpable(ch: char) -> bool {
    ch.is_ascii_lowercase() || "'`<>".contains(ch)
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
                from,
                to,
                edits,
                text,
                ..
            } = event
            {
                self.batches.push(Batch {
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

/// The marks one editor's vim state holds: `a` to `z`, and the jump mark.
#[derive(Clone, Debug, Default)]
pub(crate) struct Marks {
    named: BTreeMap<char, Place>,
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
}

impl Marks {
    /// Starts following `model`'s buffer, before a mark is set.
    pub(crate) fn watch(&mut self, model: &mut EditorModel) {
        if self.feed.is_none() {
            let events = model.buffer_mut().subscribe();
            self.feed = Some(Feed::new(events));
        }
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
        let entries = self.list.iter_mut().filter_map(|entry| match entry {
            Entry::At(place) => Some(place),
            Entry::Past => None,
        });
        let places = self
            .named
            .values_mut()
            .chain(self.jump.iter_mut())
            .chain(self.previous.iter_mut())
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

    /// Before the rows from `first` through `last` go: their jumplist
    /// entries go to the row after them, which takes their place, at the
    /// same column, as nvim's do; past the last row if none does, or onto
    /// the only row left if every row goes.
    fn displace(&mut self, snapshot: &BufferSnapshot, first: u32, last: u32) {
        self.advance();
        let rows = snapshot.text().row_count();
        for entry in &mut self.list {
            let Some((row, column)) = entry.spot(snapshot) else {
                continue;
            };
            if !(first..=last).contains(&row) {
                continue;
            }
            *entry = match () {
                _ if last + 1 < rows => Entry::At(Place::on(snapshot, last + 1, column)),
                _ if first == 0 => Entry::At(Place::joined(snapshot, 0, column)),
                _ => Entry::Past,
            };
        }
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
        // Jumplist entries on the rows between go with the last row's text.
        self.advance();
        for entry in &mut self.list {
            if let Some((row, column)) = entry.spot(snapshot)
                && row > first
                && row < last
            {
                *entry = Entry::At(Place::joined(snapshot, last, column));
            }
        }
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
        for entry in &mut self.list {
            *entry = match entry.spot(snapshot) {
                Some((row, column)) => Entry::At(Place::on(snapshot, row, column)),
                None => Entry::Past,
            };
        }
        let follow = |place: &Place| {
            let (row, column) = place.position(snapshot)?;
            Some(Place::on(snapshot, row, column))
        };
        self.update(follow);
        let entries = self.list.iter().any(|entry| matches!(entry, Entry::At(_)));
        if self.named.is_empty() && self.jump.is_none() && self.previous.is_none() && !entries {
            self.feed = None;
        }
    }

    /// Each mark becomes what `f` makes of it, or goes; a jumplist entry
    /// `f` would delete stays as it is.
    fn update(&mut self, f: impl Fn(&Place) -> Option<Place>) {
        self.advance();
        let named = std::mem::take(&mut self.named);
        self.named = named
            .into_iter()
            .filter_map(|(name, place)| Some((name, f(&place)?)))
            .collect();
        self.jump = self.jump.as_ref().and_then(&f);
        self.previous = self.previous.as_ref().and_then(&f);
        for entry in &mut self.list {
            if let Entry::At(place) = entry
                && let Some(moved) = f(place)
            {
                *place = moved;
            }
        }
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
    /// Followed through the batches since it was read, from `Feed`.
    Set(Place, Feed),
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
            Of::Set(place, feed) => {
                let mut place = place.clone();
                place.advance_through(&feed.batches(), snapshot.version());
                return place.position(snapshot);
            }
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
    /// How many feed consumers are registered, for tests.
    #[cfg(test)]
    pub(crate) fn marks_consumers(&self) -> usize {
        self.marks
            .feed
            .as_ref()
            .map_or(0, |feed| feed.lock().consumers.len())
    }

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
    target.state.marks.watch(target.model);
    let place = Place::at(target.model.snapshot(), at);
    let marks = &mut target.state.marks;
    match name {
        'a'..='z' => {
            marks.named.insert(name, place);
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
/// finding a bracket, a count of 100 or less, or `{ } ( )` not running out
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
