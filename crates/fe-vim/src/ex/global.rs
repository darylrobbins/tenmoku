//! `:g[lobal][!]` and `:v[global]` (`:help :g`): each row of the range (by
//! default all) that matches the pattern (with `!` or `:v`, that doesn't)
//! is marked, then the command runs with the cursor on each marked row that
//! still exists, in order, the whole as one undo step ([`Group`]).
//!
//! A row stops existing as nvim's marked lines do: deleted (its text and a
//! line break taken out, by any command), joined into the row above (`J`,
//! `:j`, a visual `J`, or `.` repeating one), moved by `:m` (rows it passes
//! over keep theirs), or sorted by `:sort` (every row of its range, even
//! left as it was). Read from each edit fe-vim makes: a line break taken
//! out with a whole row is that row's (`dd`), else the row after it joins
//! the one before; but in a join's edits every line break taken out joins
//! the row after it, even to an empty row, and `:m` and `:sort` (which
//! rewrite rows in place) say what they did. The rest follow the rows put
//! in or taken out above them, and text put in at a row's start (`P`, `O`)
//! goes above it.
//!
//! A shell command it runs (`:g/x/.!sort`, `:g/x/r !date`) is refused,
//! whoever runs the `:g` (`ex/filter.rs`).

use std::{cell::RefCell, ops::RangeInclusive};

use fe_buffer::{BufferEvent, Edit, Origin, Subscription, TransactionId, Version};
use fe_command::{Actor, Dispatched, Effect};
use fe_editor::commands::origin;
use fe_text::Text;

use super::{
    Ex, ExCommand, ExError, Handled, LineSearch, execute, first_non_blank, split, to_row_start,
};
use crate::{
    VimTarget, history, insert,
    marks::Place,
    operator,
    search::{self, Deadline, Pattern, PatternError},
    substitute,
};

/// What a `:g` or `:normal` running on this thread shares with the commands
/// it runs (each of those may be another ex line, or keys).
#[derive(Default)]
struct Running {
    /// While a `:g` runs, what the commands it runs said they did since its
    /// marks last caught up.
    notes: Option<Vec<Note>>,
    /// How many [`Group`]s are open, each inside the one before.
    groups: u32,
    /// The undo step of the outermost `:g` or `:normal` running.
    group: Option<Grouped>,
    /// How many `:normal`s are running, each inside the one before.
    normals: u32,
}

thread_local! {
    static RUNNING: RefCell<Running> = RefCell::default();
}

/// A command that took the buffer from version `from` to `until`, and
/// what it did there that its edits can't tell.
struct Note {
    from: Version,
    until: Version,
    change: Change,
}

/// What a command noted it did.
pub(super) enum Change {
    /// `:m` moved `rows` below line `to`.
    Moved { rows: RangeInclusive<u32>, to: u32 },
    /// `:sort` sorted `rows`, which are `by` more after it (fewer, with
    /// `u`).
    Sorted { rows: RangeInclusive<u32>, by: i64 },
    /// Rows were joined: each line break taken out joins the row after it
    /// to the one before.
    Joined,
}

/// Whether `name` (a command's letters) is `:g[lobal]` (`Some(false)`) or
/// `:v[global]` (`Some(true)`).
pub(super) fn kind(name: &str) -> Option<bool> {
    let is = |full: &str| !name.is_empty() && full.starts_with(name);
    match name {
        _ if is("global") => Some(false),
        _ if is("vglobal") => Some(true),
        _ => None,
    }
}

/// A `:g` (or, if `vglobal`, a `:v`) line, from what follows its name:
/// `[!]`, blanks, then the pattern between delimiters (any character but
/// a letter; `\/`, `\?` and `\&` for the last search's and substitute's
/// patterns), then the command; `check` is `ex::command`'s.
pub(super) fn parse(
    vglobal: bool,
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    let (bang, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    if bang && vglobal {
        return Err(ExError::NoBang);
    }
    check(true)?;
    let args = args.trim_start_matches([' ', '\t']);
    let (pattern, command) = match args.chars().next() {
        None => return Err(ExError::NoGlobalPattern),
        Some('\\') => {
            let pattern = match args[1..].chars().next() {
                Some('/') => LineSearch::LastSearch { forward: true },
                Some('?') => LineSearch::LastSearch { forward: false },
                Some('&') => LineSearch::LastSubstitute,
                _ => return Err(ExError::BackslashDelimiter),
            };
            (pattern, &args[2..])
        }
        Some(ch) if ch.is_ascii_alphabetic() => return Err(ExError::LetterDelimiter),
        Some(delimiter) => {
            let rest = &args[delimiter.len_utf8()..];
            let (pattern, command) = search::split_search(rest, delimiter);
            let pattern = LineSearch::Pattern {
                pattern,
                forward: true,
            };
            (pattern, command.unwrap_or(""))
        }
    };
    Ok(ExCommand::Global {
        invert: bang || vglobal,
        pattern,
        command: command.to_string(),
    })
}

/// Whether `command` (a `:g`'s) is another `:g`.
pub(super) fn nested(command: &str) -> bool {
    let (_, _, rest) = split(command);
    let name_len = rest
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    kind(&rest[..name_len]).is_some()
}

/// Whether a `:g` or a `:normal` is running, which runs no shell command
/// (`ex/filter.rs`), whoever asked for it.
pub(super) fn running() -> bool {
    RUNNING.with_borrow(|running| running.notes.is_some() || running.normals > 0)
}

/// Notes for a `:g` running that a command did `change`, taking the buffer
/// from version `from` to `until`, where its edits can't tell (`:m` and
/// `:sort` rewrite rows in place; a join's edits are a delete's).
pub(super) fn noted(from: Version, until: Version, change: Change) {
    RUNNING.with_borrow_mut(|running| {
        if let Some(notes) = &mut running.notes
            && from != until
        {
            notes.push(Note {
                from,
                until,
                change,
            });
        }
    });
}

/// `:g`: runs `command` on each row of `rows` that `pattern` matches (or,
/// if `invert`, doesn't) and still exists, as the module describes.
pub(super) fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    invert: bool,
    pattern: &LineSearch,
    command: &str,
) -> Result<Handled, ExError> {
    if RUNNING.with_borrow(|running| running.notes.is_some()) {
        return Err(ExError::Recursive);
    }
    let (pattern, parsed) = searched(target, pattern)?;
    target.model.sync_display();
    // Every row is read before anything runs: past the time limit, nothing
    // does, as for `:s`.
    let marked: Vec<u32> = {
        let text = target.model.text();
        let deadline = Deadline::new();
        let mut marked = Vec::new();
        for row in rows {
            let found = search::first_in_row(text, row, &parsed, &deadline)
                .map_err(|interrupted| ExError::Pattern(interrupted.to_string()))?;
            if found.is_some() != invert {
                marked.push(row);
            }
        }
        marked
    };
    if marked.is_empty() {
        // As in nvim, a message but no error.
        target.state.message = Some(match invert {
            false => format!("Pattern not found: {pattern}"),
            true => format!("Pattern found in every line: {pattern}"),
        });
        return Ok(Ok(Dispatched::new(Effect::Notify)));
    }

    // `:s` finding nothing on a row isn't an error here, as in nvim.
    let substitute = matches!(
        Ex::parse(command),
        Ok(Ex {
            command: ExCommand::Substitute(_),
            ..
        })
    );
    // No command is `:p`, which (with nowhere to print) only moves the
    // cursor.
    let print = command.trim_start_matches([' ', '\t', ':']).is_empty();
    let home = {
        let model = &target.model;
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column as usize)
    };
    let group = Group::begin(target, actor);
    let busy = Busy::start();
    // Its jumplist entry, where it began, follows what its commands do;
    // their own jumps add none.
    target.state.marks.watch(target.model);
    let from = target.model.primary_selection().head();
    target
        .state
        .marks
        .add_jump(Place::at(target.model.snapshot(), from));
    target.state.marks.keep_jumps(true);
    let mut marks = Marks::new(target, marked, home.0);
    let mut visited = None;
    let mut outcome = Ok(());
    while let Some(row) = marks.next() {
        to_row_start(target, row, true);
        visited = Some(row);
        if print {
            continue;
        }
        let before = history::pairs(target.model);
        target.state.failed = false;
        let ran = execute(target, actor, command);
        Group::note(target, before);
        marks.catch_up();
        match ran {
            Ok(Ok(_)) if !target.state.failed => {}
            Err(ExError::NotFound(_)) if substitute => {}
            // A failed command leaves its message, and the rest don't run.
            Ok(Ok(_)) => break,
            Ok(Err(error)) => {
                outcome = Err(Err(error));
                break;
            }
            Err(error) => {
                outcome = Err(Ok(error));
                break;
            }
        }
    }
    drop(busy);
    target.state.marks.keep_jumps(false);
    let transactions: Vec<TransactionId> = group.end(target).into_iter().collect();
    // As in nvim, the jump mark is where the cursor was before, followed
    // to where its row went (deleted, to the row that took its place):
    // commands run here don't set it.
    target.model.sync_display();
    let snapshot = target.model.snapshot().clone();
    let text = snapshot.text();
    let row = marks.home().min(text.row_count() - 1);
    let column = home.1.min(text.row_len(row) as usize);
    let place = Place::at(
        &snapshot,
        text.clip_offset(text.row_start(row) + column, fe_text::Bias::Left),
    );
    target.state.marks.watch(target.model);
    target.state.marks.set_jump(place);
    if let (true, Some(row)) = (print, visited) {
        let caret = first_non_blank(text, row.min(text.row_count() - 1));
        target.model.set_selections([operator::caret(caret)]);
        target.state.goals = None;
    }
    let dispatched = Dispatched::new(Effect::Changed).with_transactions(transactions);
    match outcome {
        Ok(()) => Ok(Ok(dispatched)),
        Err(Ok(error)) => {
            target.state.fail(error.to_string());
            Ok(Ok(dispatched))
        }
        Err(Err(error)) => Ok(Err(error)),
    }
}

/// The pattern `search` names (empty: the last search's) and it parsed,
/// made the last search as `/` would, keeping its direction, and the
/// highlight.
fn searched(target: &mut VimTarget<'_>, search: &LineSearch) -> Result<(String, Pattern), ExError> {
    let registers = &mut target.registers;
    let last_search = registers
        .last_search()
        .map(|(pattern, _)| pattern.to_string());
    let pattern = match search {
        LineSearch::LastSubstitute => registers
            .substitute
            .pattern()
            .ok_or(ExError::NoPreviousSubstitute)?
            .to_string(),
        LineSearch::LastSearch { .. } | LineSearch::Repeat { .. } => {
            last_search.ok_or(ExError::NoPreviousSearch)?
        }
        LineSearch::Pattern { pattern, .. } if pattern.is_empty() => {
            last_search.ok_or(ExError::NoPreviousSearch)?
        }
        LineSearch::Pattern { pattern, .. } => pattern.clone(),
    };
    let forward = registers.last_search().is_none_or(|(_, forward)| forward);
    let keep = !matches!(search, LineSearch::LastSearch { .. });
    match Pattern::parse_with(&pattern, registers.ignores_case(&pattern, false)) {
        Ok(parsed) => {
            if keep {
                registers.set_search(&pattern, forward);
                super::note_address_pattern(registers, search);
            }
            target.state.highlight = Some(parsed.clone());
            Ok((pattern, parsed))
        }
        Err(error) => {
            // Vim rejects it, but it's still the last search.
            if keep && matches!(error, PatternError::Invalid(_)) {
                registers.set_search(&pattern, forward);
                super::note_address_pattern(registers, search);
                target.state.highlight = None;
            }
            Err(ExError::Pattern(error.to_string()))
        }
    }
}

/// A `:g` running, for the commands it runs: another `:g` is E147, a `:s`
/// can't ask (`c`), and what they do is noted ([`noted`]).
struct Busy {
    _unasked: substitute::Unasked,
}

impl Busy {
    fn start() -> Self {
        RUNNING.with_borrow_mut(|running| running.notes = Some(Vec::new()));
        Self {
            _unasked: substitute::Unasked::start(),
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        RUNNING.with_borrow_mut(|running| running.notes = None);
    }
}

/// The marked rows not yet run on, followed through the edits since they
/// were marked, and the row the cursor began on.
struct Marks {
    /// Each one's row, less the [`Shifts`] up to it.
    rows: Vec<i64>,
    alive: Vec<bool>,
    shifts: Shifts,
    /// The first not yet run on.
    next: usize,
    /// The cursor's row before the `:g`: it's never deleted, but goes to
    /// the row that took its place.
    home: i64,
    events: Subscription,
    /// The text before the next event's edits.
    text: Text,
}

impl Marks {
    fn new(target: &mut VimTarget<'_>, rows: Vec<u32>, home: u32) -> Self {
        let events = target.model.buffer_mut().subscribe();
        let text = target.model.buffer().text().clone();
        Self {
            alive: vec![true; rows.len()],
            shifts: Shifts(vec![0; rows.len() + 1]),
            rows: rows.into_iter().map(i64::from).collect(),
            next: 0,
            home: i64::from(home),
            events,
            text,
        }
    }

    fn row(&self, ix: usize) -> i64 {
        self.rows[ix] + self.shifts.sum(ix)
    }

    fn set_row(&mut self, ix: usize, row: i64) {
        self.rows[ix] = row - self.shifts.sum(ix);
    }

    /// The first mark not yet run on whose row is `row` or after.
    fn first_from(&self, row: i64) -> usize {
        let (mut low, mut high) = (self.next, self.rows.len());
        while low < high {
            let mid = (low + high) / 2;
            match self.row(mid) < row {
                true => low = mid + 1,
                false => high = mid,
            }
        }
        low
    }

    /// The next marked row that still exists, now run on.
    fn next(&mut self) -> Option<u32> {
        while self.next < self.rows.len() {
            let ix = self.next;
            self.next += 1;
            if self.alive[ix] {
                return Some(u32::try_from(self.row(ix)).unwrap_or(0));
            }
        }
        None
    }

    fn home(&self) -> u32 {
        u32::try_from(self.home).unwrap_or(0)
    }

    /// Follows the edits since the last call.
    fn catch_up(&mut self) {
        let notes = RUNNING.with_borrow_mut(|running| {
            running
                .notes
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        });
        for event in self.events.drain() {
            let BufferEvent::Edited {
                from,
                to,
                edits,
                text,
                ..
            } = event
            else {
                continue;
            };
            let note = notes
                .iter()
                .find(|note| note.from <= from && to <= note.until);
            match note.map(|note| (&note.change, to == note.until)) {
                Some((Change::Joined, _)) => self.edited(&edits, &text, true),
                Some((Change::Moved { rows, to }, true)) => self.move_rows(rows, *to),
                Some((Change::Sorted { rows, by }, true)) => self.sort_rows(rows, *by),
                Some(_) => {}
                None => self.edited(&edits, &text, false),
            }
            self.text = text;
        }
    }

    /// Follows `:m` moving its rows: they lose their marks, and the rows
    /// between them and where they go shift.
    fn move_rows(&mut self, rows: &RangeInclusive<u32>, to: u32) {
        let (first, last) = (i64::from(*rows.start()), i64::from(*rows.end()));
        let (count, to) = (last - first + 1, i64::from(to));
        let block = self.first_from(first)..self.first_from(last + 1);
        if to <= first {
            // Up: the rows from `to` go down below them.
            let shifted = self.first_from(to)..block.start;
            self.shifts.add(shifted.start, count);
            self.shifts.add(shifted.end, -count);
            for ix in block {
                self.alive[ix] = false;
                self.set_row(ix, last);
            }
            self.home = match self.home {
                home if (first..=last).contains(&home) => to + home - first,
                home if (to..first).contains(&home) => home + count,
                home => home,
            };
        } else {
            // Down, below line `to`: the rows up to it go up above them.
            let shifted = block.end..self.first_from(to);
            self.shifts.add(shifted.start, -count);
            self.shifts.add(shifted.end, count);
            for ix in block {
                self.alive[ix] = false;
                self.set_row(ix, first);
            }
            self.home = match self.home {
                home if (first..=last).contains(&home) => to - count + home - first,
                home if (last + 1..to).contains(&home) => home - count,
                home => home,
            };
        }
    }

    /// Follows `:sort` over `rows`, `by` more after it: their marks go, and
    /// the rows after them shift.
    fn sort_rows(&mut self, rows: &RangeInclusive<u32>, by: i64) {
        let (first, last) = (i64::from(*rows.start()), i64::from(*rows.end()));
        let block = self.first_from(first)..self.first_from(last + 1);
        self.shifts.add(block.end, by);
        for ix in block {
            self.alive[ix] = false;
            self.set_row(ix, first);
        }
        self.home = match self.home {
            home if home > last => home + by,
            home if home >= first => home.min(last + by),
            home => home,
        };
    }

    /// Follows one edit batch, from `self.text` to `after`; in a join's,
    /// each line break taken out joins the row after it.
    fn edited(&mut self, edits: &[Edit], after: &Text, joined: bool) {
        let before = &self.text.clone();
        // Each edit's first and last rows, and how many rows it adds.
        let spans: Vec<(i64, i64, i64)> = edits
            .iter()
            .map(|edit| {
                let row = |text: &Text, offset| i64::from(text.offset_to_point(offset).row);
                let (first, last) = (row(before, edit.old.start), row(before, edit.old.end));
                let added = row(after, edit.new.end) - row(after, edit.new.start);
                (first, last, added - (last - first))
            })
            .collect();
        // The marks on rows an edit touches, as they were: found before
        // the shifts below change where the rest are.
        let mut touched: Vec<(usize, i64)> = Vec::new();
        let mut ends = Vec::with_capacity(spans.len());
        for &(first, last, _) in &spans {
            let end = self.first_from(last + 1);
            let start = self
                .first_from(first)
                .max(touched.last().map_or(0, |&(ix, _)| ix + 1));
            touched.extend((start..end).map(|ix| (ix, self.row(ix))));
            ends.push(end);
        }
        let home = self.home;
        let mut home_shift = 0;
        let mut home_touched = false;
        for (&(first, last, by), end) in spans.iter().zip(ends) {
            self.shifts.add(end, by);
            match home {
                home if home > last => home_shift += by,
                home if home >= first => home_touched = true,
                _ => {}
            }
        }
        let row_of = |row: i64| {
            let start = before.row_start(u32::try_from(row).unwrap_or(0));
            i64::from(after.offset_to_point(follow(edits, start)).row)
        };
        self.home = match home_touched {
            true => row_of(home),
            false => home + home_shift,
        };
        for (ix, row) in touched {
            if self.alive[ix] && gone(before, edits, row, joined) {
                self.alive[ix] = false;
            }
            let row = row_of(row);
            self.set_row(ix, row);
        }
    }
}

/// Whether `row` of `before` stops existing in `edits`: deleted (its whole
/// text and a line break taken out), or joined into the row above (the line
/// break before it taken out, unless only whole rows above it were deleted
/// with it, nothing put in; in a join's edits, `joined`, always).
fn gone(before: &Text, edits: &[Edit], row: i64, joined: bool) -> bool {
    let row = u32::try_from(row).unwrap_or(0);
    let last = before.row_count() - 1;
    let extent = |row: u32| {
        let start = before.row_start(row);
        (start, start + before.row_len(row) as usize)
    };
    let (start, end) = extent(row);
    let break_before = row > 0 && removed(edits, start - 1);
    if joined {
        return break_before;
    }
    let break_after = row < last && removed(edits, end);
    // Rows above deleted whole, with the line break between them: the edit
    // starts at a row's start (else what's left of that row takes this
    // one's text), ends at this one's (else this one is what's left) and
    // puts nothing in.
    let above_deleted = break_before
        && at_or_before(edits, start - 1).is_some_and(|edit| {
            edit.new.is_empty()
                && edit.old.end == start
                && before.offset_to_point(edit.old.start).column == 0
        });
    let break_before = break_before && !above_deleted;
    (whole(edits, start, end) && break_after) || break_before
}

/// The last edit starting at or before `offset`.
fn at_or_before(edits: &[Edit], offset: usize) -> Option<&Edit> {
    let ix = edits.partition_point(|edit| edit.old.start <= offset);
    ix.checked_sub(1).map(|ix| &edits[ix])
}

/// Whether the character at `offset` is taken out.
fn removed(edits: &[Edit], offset: usize) -> bool {
    at_or_before(edits, offset).is_some_and(|edit| offset < edit.old.end)
}

/// Whether all of `start..end` is taken out (an empty extent, if text
/// around it is).
fn whole(edits: &[Edit], start: usize, end: usize) -> bool {
    at_or_before(edits, start)
        .is_some_and(|edit| edit.old.start < edit.old.end && end <= edit.old.end)
}

/// Where `offset`, a row's start, is after `edits`: text put in right there
/// goes before it, and in text taken out it goes to where that was.
fn follow(edits: &[Edit], offset: usize) -> usize {
    let ix = edits.partition_point(|edit| {
        edit.old.end < offset || (edit.old.end == offset && edit.old.start < offset)
    });
    match edits.get(ix) {
        Some(edit) if edit.old.start == edit.old.end && edit.old.start == offset => edit.new.end,
        Some(edit) if edit.old.start <= offset => edit.new.start,
        _ => match ix.checked_sub(1) {
            Some(ix) => offset - edits[ix].old.end + edits[ix].new.end,
            None => offset,
        },
    }
}

/// Row shifts over the marks, a Fenwick tree: a mark's shift is the sum of
/// those added at it and before.
struct Shifts(Vec<i64>);

impl Shifts {
    /// Shifts the marks from `ix` on by `by`.
    fn add(&mut self, ix: usize, by: i64) {
        let mut at = ix + 1;
        while at < self.0.len() {
            self.0[at] += by;
            at += at.isolate_lowest_one();
        }
    }

    fn sum(&self, ix: usize) -> i64 {
        let mut at = ix + 1;
        let mut sum = 0;
        while at > 0 {
            sum += self.0[at];
            at -= at.isolate_lowest_one();
        }
        sum
    }
}

/// Carets as rows and byte columns.
type Carets = Vec<(u32, usize)>;

/// The undo step of the outermost `:g` or `:normal` running, and where `u`
/// and `<C-r>` put the carets after it, as nvim's do: where the first
/// command that changed the text noted them (or, for one that noted none,
/// where they were before it).
struct Grouped {
    /// Its transaction: a new one once an undo or redo run in it closed the
    /// one before.
    id: TransactionId,
    origin: Origin,
    /// The buffer's version where the transaction began.
    version: Version,
    /// Where `u` and `<C-r>` put them.
    carets: Option<(Carets, Carets)>,
    /// An insert session's, noted before it typed anything.
    session: Option<Carets>,
}

/// One undo step for the commands a `:g` or `:normal` runs: a transaction
/// they all join, opened by [`Group::begin`] and closed by [`Group::end`].
pub(super) struct Group {
    owner: bool,
}

impl Group {
    pub(super) fn begin(target: &mut VimTarget<'_>, actor: &Actor) -> Self {
        insert::adopt_closing(target.state);
        target.model.sync_display();
        let (id, version) = {
            let mut buffer = target.model.buffer_mut();
            let version = buffer.version();
            (buffer.start_transaction(origin(actor)), version)
        };
        let owner = RUNNING.with_borrow_mut(|running| {
            running.groups += 1;
            let owner = running.group.is_none();
            if owner {
                running.group = Some(Grouped {
                    id,
                    origin: origin(actor),
                    version,
                    carets: None,
                    session: None,
                });
            }
            owner
        });
        Self { owner }
    }

    /// Whether this is the outermost `:g` or `:normal` running.
    pub(super) fn owner(&self) -> bool {
        self.owner
    }

    /// After a command or key run in a group, which began with the carets
    /// at `before`. An undo or redo closes the transaction (nvim's undoes
    /// what the group did so far): what follows goes in a new one, opened
    /// as deep, the group's undo step from then on.
    pub(super) fn note(target: &mut VimTarget<'_>, before: Vec<(u32, usize)>) {
        RUNNING.with_borrow_mut(|running| {
            let depth = running.groups;
            let Some(group) = &mut running.group else {
                return;
            };
            {
                let mut buffer = target.model.buffer_mut();
                let id = buffer.start_transaction(group.origin.clone());
                if id == group.id {
                    buffer.end_transaction();
                } else {
                    for _ in 1..depth {
                        buffer.start_transaction(group.origin.clone());
                    }
                    *group = Grouped {
                        id,
                        origin: group.origin.clone(),
                        version: buffer.version(),
                        carets: None,
                        session: None,
                    };
                }
            }
            let version = target.model.buffer().version();
            if group.carets.is_some() {
                return;
            }
            let carets = &mut target.state.carets;
            // Its insert session closing inside the group drops it.
            if let Some((id, session)) = carets.take_pending() {
                if id == group.id {
                    group.session = Some(session.clone());
                }
                carets.record_pending(id, session);
            }
            if version == group.version {
                return;
            }
            let undo = match carets.get(group.id) {
                Some(noted) => noted.to_vec(),
                None => group.session.take().unwrap_or(before),
            };
            let redo = carets
                .get_redo(group.id)
                .map_or_else(|| undo.clone(), <[_]>::to_vec);
            group.carets = Some((undo, redo));
        });
    }

    /// Closes the transaction, returning it once the outermost closes, if
    /// it changed anything.
    pub(super) fn end(self, target: &mut VimTarget<'_>) -> Option<TransactionId> {
        let ended = target.model.buffer_mut().end_transaction();
        if self.owner
            && let Some(Grouped {
                carets: Some((undo, redo)),
                ..
            }) = RUNNING.with_borrow_mut(|running| running.group.take())
            && let Some(id) = ended
        {
            let carets = &mut target.state.carets;
            carets.settle(Some((id, undo)), Some(id));
            carets.record_redo(id, redo);
        }
        ended
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        RUNNING.with_borrow_mut(|running| {
            running.groups -= 1;
            if self.owner {
                running.group = None;
            }
        });
    }
}

/// Counts a `:normal` running until dropped, refusing one more than nvim's
/// 'maxmapdepth' (1000) would allow: E192. Far fewer here, as each level's keys
/// run on this thread's stack.
pub(super) struct Depth;

/// How many `:normal`s may run inside each other. A level uses about 17-20 KiB
/// of a debug build's stack on Linux (25 levels fit 512 KiB, 30 don't), and
/// more on macOS: 5 leaves over 4x headroom in a 512 KiB thread.
const NORMAL_DEPTH: u32 = 5;

impl Depth {
    pub(super) fn enter() -> Result<Self, ExError> {
        RUNNING.with_borrow_mut(|running| match running.normals < NORMAL_DEPTH {
            true => {
                running.normals += 1;
                Ok(Self)
            }
            false => Err(ExError::NormalTooDeep),
        })
    }
}

impl Drop for Depth {
    fn drop(&mut self) {
        RUNNING.with_borrow_mut(|running| running.normals -= 1);
    }
}
