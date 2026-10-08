//! Insert sessions: `vim.insert` (`i a I A o O`), the change operator `c`,
//! and `vim.normal_mode`, which ends them.
//!
//! The command that enters insert mode opens one `fe_buffer` transaction
//! with its actor's origin and, at each caret, anchors around where the
//! typed text goes. Text typed meanwhile arrives as ordinary editor input
//! (`EditorModel::insert` in the real app) and joins the transaction, its
//! origin matching; another origin's edits get their own. `vim.normal_mode`
//! repeats the typed text for a count, drops the indentation the session
//! made that went unused, ends the transaction, steps the caret back one
//! grapheme and returns to normal mode, keeping the typed text for `.`.
//!
//! The session follows its indentation byte by byte through every edit
//! made while it's open ([`Indents`]), so `Esc` removes nothing else: no
//! other origin's text, and no blanks that were already there but those a
//! linewise `c` kept as its row's indentation. Positions an edit leaves are
//! taken in the snapshot right after it, since catching the model up
//! measures text, which may edit the buffer through another model.

use std::{
    collections::HashMap,
    fmt,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use fe_buffer::{
    Anchor, BufferEvent, BufferSnapshot, Edit, EditKind, Origin, ResolvedSelection, Subscription,
    TransactionId, Version,
};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{
    EditorModel,
    commands::{EditorHost, origin},
};
use fe_text::Text;

use crate::{
    Change, InsertArgs, InsertAt, LastChange, Mode, Motion, OperateArgs, RangeClass, Register,
    Registers, Target, VimState, VimTarget, Write, history,
    motion::Cursor,
    operator::{MAX_PUT, Op, Span, apply, caret, grapheme_len, merge, row_end, sync},
};

/// An open insert session.
#[derive(Clone, Debug)]
pub(crate) struct Session {
    /// One per caret, in order: the last is the primary caret's.
    regions: Vec<Region>,
    /// Shared by clones of the state, so they agree.
    ledger: Arc<Mutex<Ledger>>,
    origin: Origin,
    /// How many times the typed text goes in.
    count: u32,
    /// The change that opened it, kept for `.` with the typed text.
    change: Change,
    /// `o` and `O`: a count opens more rows.
    opens: bool,
    /// It continues a session a split ended ([`VimState::split_session`]).
    resumed: bool,
}

/// What an open session follows: the buffer's edits since it opened, and
/// the indentation it made, as of the last batch of them followed. It's
/// locked only between the model's catch-ups, never across one: measuring
/// text as the model catches up may stamp the context through a clone of
/// the state, which follows the buffer's edits here.
struct Ledger {
    events: Subscription,
    indents: Indents,
    /// The versions the line breaks typed through [`VimState::line_break`]
    /// started from, until followed.
    breaks: Vec<Version>,
    /// The count's copies `vim.normal_mode` puts in, until followed.
    copies: Option<Copied>,
    /// How far ending the session has got, through this state or a clone.
    end: End,
}

/// How far ending a session has got.
#[derive(Debug)]
enum End {
    Open,
    /// `vim.normal_mode` is ending it, and lets go of the record while the
    /// model catches up.
    Ending,
    /// It ended, leaving this: another ending it only takes this.
    Ended(Closed),
}

/// A session this state left to a clone that was ending it, until the
/// outcome is published and adopted.
#[derive(Clone, Debug)]
pub(crate) struct Closing {
    ledger: Arc<Mutex<Ledger>>,
    replaying: bool,
}

/// Adopts the outcome of a session a clone ended, if it's published: the
/// last change for `.` and the carets record for undo and redo. Called
/// before the commands that read them.
pub(crate) fn adopt_closing(state: &mut VimState) {
    let Some(closing) = &state.closing else {
        return;
    };
    let closed = match &closing
        .ledger
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .end
    {
        End::Ended(closed) => closed.clone(),
        _ => return,
    };
    let replaying = closing.replaying;
    state.closing = None;
    state.last_change = Some(closed.last_change);
    if !replaying {
        state.carets.close(closed.transaction);
    }
}

/// What ending a session left: the last change for `.`, and the
/// transaction it closed, if that was kept, for the carets record.
#[derive(Clone, Debug)]
struct Closed {
    last_change: LastChange,
    transaction: Option<TransactionId>,
}

/// The count's copies, for [`Ledger::follow`]: the version their batch
/// starts from, and each caret's. They go in without the indentation `Esc`
/// would remove from them where they decide that themselves (see
/// [`copy_of`]), so what's noted takes as little for a count of a million
/// as for two.
struct Copied {
    from: Version,
    copies: Vec<Copies>,
}

/// One caret's copies of the text it typed, for the session.
struct Copies {
    /// Where they start, in the text right after their batch.
    start: usize,
    /// Whether the first types on the row it starts on: at the caret, if
    /// the typed text's first row isn't empty; never for `o` and `O`.
    types: bool,
    /// The indentation left in them that `Esc` may remove, from `start`:
    /// on the last copy's last row, which the text after it continues.
    indents: Vec<Indent>,
}

/// A copy of a caret's typed text for a count, as it goes in: see
/// [`copy_of`].
struct Copy {
    /// Each copy but the last.
    each: String,
    /// The last copy, which keeps the indentation on its last row.
    last: String,
    /// That indentation, from the last copy's start.
    open: Vec<Indent>,
}

/// The rows `o` and `O` open for a count's copies: each is indented with
/// spaces as wide as the row before's indentation, as typing the text again
/// would, unless that indentation would go unused.
#[derive(Copy, Clone, Debug)]
struct Rows {
    /// Whether they're indented at all: only if the row they repeat was
    /// typed on or holds more than blanks.
    indented: bool,
    /// The width of the row before's indentation.
    width: usize,
    tab: usize,
}

#[cfg(test)]
thread_local! {
    /// How many entries the last count's copies were noted with on this
    /// thread: see [`Copied::entries`].
    pub(crate) static COPIES_NOTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl fmt::Debug for Ledger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ledger")
            .field("indents", &self.indents)
            .finish_non_exhaustive()
    }
}

/// One caret's part of a session: the text typed at it lies between two
/// anchors, so another origin's edits outside them and a caret moved
/// elsewhere don't count as typed there.
#[derive(Clone, Debug)]
struct Region {
    /// Where the typed text starts: text typed at it goes after it.
    start: Anchor,
    /// Where it ends: text typed at it goes before it.
    end: Anchor,
}

/// What a command run inside an insert session may do to it
/// ([`VimState::split_session`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Splits {
    /// It may edit the buffer: the session ends before it.
    Edits,
    /// It moves carets or changes selections, as a cursor key or a mouse
    /// click does: the session ends after it only if it did.
    Moves,
    /// Any other command that isn't text input, such as copy, save or a
    /// fold: the session ends after it, whatever it did.
    Other,
}

impl VimState {
    /// Runs `command`, which is neither typing nor `Esc`, splitting an open
    /// insert session around it, as the cursor keys split a session in Vim
    /// (`:help ins-special-special`): in the app, an editor command other
    /// than text input, or a mouse click. `splits` says how it splits. Outside insert mode it only runs `command`.
    ///
    /// The session ends as `Esc` ends it, but for the count's copies (nvim
    /// drops the count) and the step back: the indentation it made that
    /// went unused goes, its transaction closes, and `.` keeps its change and
    /// the text typed up to the primary caret. The mode stays insert, and a
    /// new session with the same origin opens at the carets `command` left,
    /// as `i` opens one, keeping any selection. So the text typed next is an
    /// undo step of its own, which `.` repeats as `i` would; if the new
    /// session changes nothing, `.` keeps the change before the split.
    ///
    /// A command that [edits](Splits::Edits) runs once the session has
    /// ended, so its edits are their own. Any other runs inside the session,
    /// which ends after it if it [moved](Splits::Moves) a caret or changed
    /// a selection (or is [another](Splits::Other) command), and keeps the
    /// indentation on the rows the carets are on then, as nvim keeps it
    /// while the cursor stays on its row (`o<Left>`) and drops it once it
    /// leaves (`o<Up>`).
    pub fn split_session<R>(
        &mut self,
        model: &mut EditorModel,
        splits: Splits,
        command: impl FnOnce(&mut EditorModel) -> R,
    ) -> R {
        let Some(origin) = self.session.as_ref().map(|session| session.origin.clone()) else {
            return command(model);
        };
        if splits == Splits::Edits {
            end(model, self, Ending::Split);
            let result = command(model);
            resume(model, self, origin);
            return result;
        }
        sync(model);
        let before = model.selections();
        let primary = model
            .snapshot()
            .anchor_before(model.primary_selection().head());
        let result = command(model);
        if splits == Splits::Other || model.selections() != before {
            end(model, self, Ending::Moved(primary));
            resume(model, self, origin);
        }
        result
    }
}

/// Opens the session a split resumes ([`VimState::split_session`]): typing
/// starts at each selection's start, where typing over it goes, and the
/// selections stay.
fn resume(model: &mut EditorModel, state: &mut VimState, origin: Origin) {
    model.sync_display();
    let text = model.text().clone();
    let mut heads: Vec<usize> = model
        .selections()
        .iter()
        .map(|selection| selection.range.start)
        .collect();
    heads.sort_unstable();
    heads.dedup();
    start(model, state, origin.clone(), &text, heads.iter().copied());
    let opened = {
        let mut buffer = model.buffer_mut();
        (buffer.snapshot(), buffer.subscribe())
    };
    let opening = Opening {
        origin,
        count: 1,
        change: Change::Insert(InsertArgs {
            at: InsertAt::Before,
            count: None,
        }),
        opens: false,
        resumed: true,
    };
    open(state, opening, &heads, Indents::default(), opened);
}

impl Session {
    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Follows the buffer's edits since the last call, so none wait to be:
    /// see [`Ledger::follow`].
    pub(crate) fn follow_edits(&self) {
        self.ledger().follow(&self.origin);
    }

    /// Notes that the batch from version `from` was a line break typed in
    /// insert mode (`EditorModel::newline`): the white space it copied
    /// after the break is the new row's indentation, which `Esc` may drop.
    /// Called before the line break is typed, and holds nothing locked once
    /// it returns, so the session can follow the batch while it's typed.
    pub(crate) fn typed_line_break(&self, from: Version) {
        self.ledger().breaks.push(from);
    }

    /// Takes back [`Session::typed_line_break`]'s note of `from` when the
    /// line break typed nothing (there was no caret to type at), so no later
    /// batch from that version is taken for it.
    pub(crate) fn forget_line_break(&self, from: Version) {
        self.ledger().breaks.retain(|&at| at != from);
    }

    /// Notes the rows the session began on as typed on, as `.` does when
    /// the change it repeats typed there (see [`LastChange`]).
    pub(crate) fn typed_on_first_rows(&self) {
        for indent in &mut self.ledger().indents.0 {
            indent.used = true;
        }
    }

    /// How many of the buffer's events wait to be followed (taking them).
    #[cfg(test)]
    pub(crate) fn backlog(&self) -> usize {
        self.ledger().events.drain().len()
    }

    /// The versions of the typed line breaks not followed yet, or `None`
    /// while the record is locked.
    #[cfg(test)]
    pub(crate) fn line_breaks(&self) -> Option<Vec<Version>> {
        match self.ledger.try_lock() {
            Ok(ledger) => Some(ledger.breaks.clone()),
            Err(std::sync::TryLockError::Poisoned(ledger)) => {
                Some(ledger.into_inner().breaks.clone())
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// Whether the record is locked, which it never is while the model
    /// catches up.
    #[cfg(test)]
    pub(crate) fn is_locked(&self) -> bool {
        matches!(
            self.ledger.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        )
    }
}

impl Ledger {
    /// Follows the buffer's edits since the last call. `origin`'s edits are
    /// typing (line breaks typed through [`VimState::line_break`] among
    /// them), but for the batch holding the count's copies, which
    /// [`Ledger::copies`] describes; anything else is another origin's,
    /// undos and redos included.
    fn follow(&mut self, origin: &Origin) {
        let events = self.events.drain();
        if matches!(self.end, End::Ended(_)) {
            return;
        }
        for event in events {
            let BufferEvent::Edited {
                origin: event_origin,
                kind,
                from,
                edits,
                text,
                ..
            } = event
            else {
                continue;
            };
            if let Some(copied) = self.copies.take_if(|copied| copied.from == from) {
                self.indents.follow(&edits, &text, Whose::Copies);
                copied.note(&mut self.indents, &text);
                self.breaks.retain(|&at| at > from);
                continue;
            }
            let whose = if &event_origin != origin || kind != EditKind::Edit {
                Whose::Other
            } else if self.breaks.contains(&from) {
                Whose::LineBreak
            } else {
                Whose::Typing
            };
            self.indents.follow(&edits, &text, whose);
            self.breaks.retain(|&at| at > from);
        }
    }
}

/// The indentation an insert session made: what `o` and `O` opened a row
/// with, what a linewise `c` (`cc`, `S`, `cj`) kept, and what a line break
/// typed in insert mode copied (through [`VimState::line_break`]). It's
/// followed through every edit while the session is open,
/// so `Esc` removes these bytes and no others, as Vim's 'autoindent'
/// removes indentation nothing was typed after (`:help 'autoindent'`). The
/// pieces are in order, never overlap and hold no line break, so each lies
/// on one row.
#[derive(Clone, Debug, Default)]
struct Indents(Vec<Indent>);

/// A piece of a session's indentation.
#[derive(Clone, Debug)]
struct Indent {
    /// Its bytes still there, in the text as of the last batch followed.
    /// Empty where `o`, `O` or a linewise `c` left a row with no
    /// indentation, which still notes whether the row was typed on.
    range: Range<usize>,
    /// Something was typed on its row since it was made, even if deleted
    /// since (but not a backspace or a line break): Vim keeps the
    /// indentation then.
    used: bool,
    /// Another origin deleted some of it or wrote inside it: it stays as it
    /// is.
    changed: bool,
}

impl Indent {
    fn new(range: Range<usize>) -> Self {
        Self {
            range,
            used: false,
            changed: false,
        }
    }

    /// The same piece, `by` bytes further on.
    fn shifted(&self, by: usize) -> Self {
        Self {
            range: self.range.start + by..self.range.end + by,
            ..self.clone()
        }
    }
}

/// Whose a batch of edits is, for [`Indents::follow`].
#[derive(Copy, Clone, PartialEq, Eq)]
enum Whose {
    /// The session's origin's: typing, `.`'s included. What it puts in is
    /// typed text, whatever it holds.
    Typing,
    /// A line break typed through [`VimState::line_break`]
    /// (`EditorModel::newline`, which copies the row's white space): the
    /// white space after the break is the new row's indentation.
    LineBreak,
    /// fe-vim's copies for a count, whose indentation [`Copied`] holds.
    Copies,
    /// Another origin's, an undo or a redo: never the session's.
    Other,
}

impl Indents {
    /// Follows one batch of `edits`, which left `text`.
    fn follow(&mut self, edits: &[Edit], text: &Text, whose: Whose) {
        let Some(first) = edits.first() else {
            return;
        };
        // Pieces ending before the first edit stay as they are.
        let ix = self
            .0
            .partition_point(|indent| indent.range.end <= first.old.start);
        for indent in self.0.split_off(ix) {
            let (pieces, touched) = survive(edits, &indent.range);
            let changed = indent.changed || (touched && whose == Whose::Other);
            self.0.extend(pieces.into_iter().map(|range| Indent {
                range,
                used: indent.used,
                changed,
            }));
        }
        if !matches!(whose, Whose::Typing | Whose::LineBreak) {
            return;
        }
        let mut typed = Vec::new();
        for edit in edits.iter().filter(|edit| !edit.new.is_empty()) {
            let inserted = text.slice_to_string(edit.new.clone());
            let indents = whose == Whose::LineBreak
                && inserted
                    .strip_prefix('\n')
                    .is_some_and(|rest| rest.chars().all(is_blank));
            match indents {
                true if edit.new.len() > 1 => {
                    let piece = Indent::new(edit.new.start + 1..edit.new.end);
                    let at = self.0.partition_point(|indent| {
                        (indent.range.start, indent.range.end)
                            < (piece.range.start, piece.range.end)
                    });
                    self.0.insert(at, piece);
                }
                true => {}
                false => typed.push((edit.new.start, inserted)),
            }
        }
        for (start, inserted) in typed {
            self.typed_on(text, start, &inserted);
        }
    }

    /// Adds `pieces`, which overlap none there.
    fn add(&mut self, pieces: impl IntoIterator<Item = Indent>) {
        self.0.extend(pieces);
        self.0
            .sort_by_key(|indent| (indent.range.start, indent.range.end));
    }

    /// Notes typing that put `inserted` at `start` in `text`: the rows it
    /// put a character on are typed on.
    fn typed_on(&mut self, text: &Text, start: usize, inserted: &str) {
        let mut at = start;
        for line in inserted.split('\n') {
            if !line.is_empty() {
                self.typed_on_row(text, at);
            }
            at += line.len() + 1;
        }
    }

    /// Notes `offset`'s row in `text` as typed on.
    fn typed_on_row(&mut self, text: &Text, offset: usize) {
        let row = row(text, offset);
        let (from, to) = (text.row_start(row), row_end(text, row));
        let low = self.0.partition_point(|indent| indent.range.start < from);
        let high = self.0.partition_point(|indent| indent.range.start <= to);
        for indent in &mut self.0[low..high] {
            indent.used = true;
        }
    }

    /// Whether anything was typed on `offset`'s row since the indentation
    /// before it there was made: for the row `o` or `O` opened, whether
    /// the count's copies keep theirs.
    fn used_before(&self, text: &Text, offset: usize) -> bool {
        let from = text.row_start(row(text, offset));
        let low = self.0.partition_point(|indent| indent.range.start < from);
        self.0[low..]
            .iter()
            .take_while(|indent| indent.range.start <= offset)
            .any(|indent| indent.used)
    }

    /// The pieces `Esc` may remove that lie in `range`, cut to it and
    /// counted from its start: copies of them go with the count's copies.
    fn removable_in(&self, range: Range<usize>) -> Vec<Indent> {
        let low = self
            .0
            .partition_point(|indent| indent.range.end <= range.start);
        self.0[low..]
            .iter()
            .take_while(|indent| indent.range.start < range.end)
            .filter(|indent| !indent.used && !indent.changed)
            .filter_map(|indent| {
                let start = indent.range.start.max(range.start);
                let end = indent.range.end.min(range.end);
                (start < end).then(|| Indent::new(start - range.start..end - range.start))
            })
            .collect()
    }

    /// What `Esc` removes: every piece nothing was typed on the row of and
    /// no other origin changed, if its row holds nothing but blanks. Never
    /// more than blanks on a row of them.
    fn unused(&self, text: &Text) -> Vec<Range<usize>> {
        let mut blank: Option<(u32, bool)> = None;
        let mut removed = Vec::new();
        for indent in &self.0 {
            if indent.used || indent.changed || indent.range.is_empty() {
                continue;
            }
            let row = row(text, indent.range.start);
            let is_blank = match blank {
                Some((at, is_blank)) if at == row => is_blank,
                _ => {
                    let is_blank = text.row_to_string(row).chars().all(is_blank);
                    blank = Some((row, is_blank));
                    is_blank
                }
            };
            if is_blank && indent.range.end <= row_end(text, row) {
                removed.push(indent.range.clone());
            }
        }
        removed
    }
}

/// What's left of `range`, in the text before a batch of `edits`, in the
/// text after it: the bytes no edit replaced, in pieces split wherever an
/// edit replaced some or put text in strictly inside. An empty range is a
/// point, which stays before text put in at it and goes to the start of a
/// replacement it was inside. Also whether any edit touched its bytes so.
fn survive(edits: &[Edit], range: &Range<usize>) -> (Vec<Range<usize>>, bool) {
    // `offset`, which lies after the first `passed` edits and before the
    // rest, in the text after them all.
    let shift = |offset: usize, passed: usize| match passed.checked_sub(1) {
        Some(ix) => offset - edits[ix].old.end + edits[ix].new.end,
        None => offset,
    };
    if range.is_empty() {
        let point = range.start;
        let ix = edits.partition_point(|edit| edit.old.end < point);
        let mapped = match edits.get(ix) {
            Some(edit) if edit.old.start < point && point < edit.old.end => edit.new.start,
            Some(edit) if edit.old.start < point => edit.new.end,
            Some(edit) if edit.old.start == point => edit.new.start,
            _ => shift(point, ix),
        };
        return (std::iter::once(mapped..mapped).collect(), false);
    }
    let mut ix = edits.partition_point(|edit| edit.old.end <= range.start);
    let mut pieces = Vec::new();
    let mut touched = false;
    let mut from = range.start;
    while let Some(edit) = edits.get(ix).filter(|edit| edit.old.start < range.end) {
        if from < edit.old.start {
            pieces.push(shift(from, ix)..shift(edit.old.start, ix));
        }
        touched = true;
        from = from.max(edit.old.end);
        ix += 1;
    }
    if from < range.end {
        pieces.push(shift(from, ix)..shift(range.end, ix));
    }
    (pieces, touched)
}

fn row(text: &Text, offset: usize) -> u32 {
    text.offset_to_point(offset).row
}

/// White space other than a line break: what indentation is made of.
fn is_blank(ch: char) -> bool {
    ch != '\n' && ch.is_whitespace()
}

/// Puts a caret at each of `heads`, offsets in `snapshot`, which the
/// model's text may have moved on from since (see [`apply`]).
fn place(model: &mut EditorModel, snapshot: &BufferSnapshot, heads: &[usize]) {
    let anchors: Vec<Anchor> = heads
        .iter()
        .map(|&head| snapshot.anchor_before(head))
        .collect();
    let heads = model.snapshot().resolve_many(&anchors);
    model.set_selections(heads.into_iter().map(caret));
}

/// `offsets`, in any order, mapped through `removed` (sorted and disjoint)
/// as deleting it moves them, in one pass over each.
fn through_deletes(offsets: &[usize], removed: &[Range<usize>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|&ix| offsets[ix]);
    let mut mapped = vec![0; offsets.len()];
    let (mut next, mut shift) = (0, 0);
    for ix in order {
        let offset = offsets[ix];
        while let Some(range) = removed.get(next).filter(|range| range.end <= offset) {
            shift += range.len();
            next += 1;
        }
        let inside = removed
            .get(next)
            .filter(|range| range.start < offset)
            .map_or(0, |range| offset - range.start);
        mapped[ix] = offset - shift - inside;
    }
    mapped
}

/// The byte length of `row`'s leading blanks.
fn indent_len(text: &Text, row: u32) -> usize {
    text.row_to_string(row)
        .bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .count()
}

/// `row`'s leading blanks as spaces, tabs expanded: what `o` and `O` copy
/// (nvim's 'autoindent' with 'expandtab').
fn spaces_of_indent(text: &Text, row: u32, tab: usize) -> String {
    let line = text.row_to_string(row);
    let width = line
        .bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .fold(0, |column, byte| match byte {
            b'\t' => column + tab - column % tab,
            _ => column + 1,
        });
    " ".repeat(width)
}

/// What opens a session, for [`enter`].
struct Opening {
    origin: Origin,
    count: u32,
    change: Change,
    opens: bool,
    resumed: bool,
}

/// Places a caret at each of `carets` (sorted, offsets in `snapshot`, the
/// text right after the opening edit), each with where the indentation the
/// session made starts, if it made any, and enters insert mode with a
/// session over them, following the buffer from `events`, subscribed at
/// `snapshot`. The transaction is open.
fn enter(
    model: &mut EditorModel,
    state: &mut VimState,
    opening: Opening,
    mut carets: Vec<(usize, Option<usize>)>,
    (snapshot, events): (BufferSnapshot, Subscription),
) {
    carets.dedup_by_key(|(head, _)| *head);
    let heads: Vec<usize> = carets.iter().map(|&(head, _)| head).collect();
    place(model, &snapshot, &heads);
    let mut indents = Indents::default();
    indents.add(
        carets
            .iter()
            .filter_map(|&(head, indent)| Some(Indent::new(indent?..head))),
    );
    open(state, opening, &heads, indents, (snapshot, events));
}

/// Enters insert mode with a session whose typing starts at each of
/// `heads` (sorted offsets in `snapshot`), following the buffer from
/// `events`, subscribed at `snapshot`. The transaction is open.
fn open(
    state: &mut VimState,
    opening: Opening,
    heads: &[usize],
    indents: Indents,
    (snapshot, events): (BufferSnapshot, Subscription),
) {
    let regions = heads
        .iter()
        .map(|&head| Region {
            start: snapshot.anchor_before(head),
            end: snapshot.anchor_after(head),
        })
        .collect();
    let ledger = Arc::new(Mutex::new(Ledger {
        events,
        indents,
        breaks: Vec::new(),
        copies: None,
        end: End::Open,
    }));
    let Opening {
        origin,
        count,
        change,
        opens,
        resumed,
    } = opening;
    state.session = Some(Session {
        regions,
        ledger,
        origin,
        count,
        change,
        opens,
        resumed,
    });
    state.mode = Mode::Insert;
    state.pending = Default::default();
    state.goals = None;
}

/// Opens the transaction a change makes, noting where it begins for each
/// caret, as offsets in `text`.
fn start(
    model: &mut EditorModel,
    state: &mut VimState,
    origin: Origin,
    text: &Text,
    begins: impl IntoIterator<Item = usize>,
) {
    let begins = begins
        .into_iter()
        .map(|offset| history::pair(text, offset))
        .collect();
    let id = model.buffer_mut().start_transaction(origin);
    state.carets.record_pending(id, begins);
}

/// `vim.insert`: moves each caret as `at` says (opening a row for `o` and
/// `O`) and starts a session.
pub(crate) fn insert(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: InsertArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    let origin = origin(actor);
    model.sync_display();
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    // Another origin's edit may have left a caret inside a grapheme: it's on
    // that grapheme.
    let heads: Vec<usize> = model
        .selections()
        .iter()
        .map(|selection| Cursor::at(&text, selection.head()).offset())
        .collect();
    // Each caret's insertion point, the text opening a row there, and how
    // far into that text the caret goes.
    let mut points: Vec<(usize, String, usize)> = heads
        .iter()
        .map(|&head| {
            let row = row(&text, head);
            let (start, end) = (text.row_start(row), row_end(&text, row));
            match args.at {
                InsertAt::Before => (head, String::new(), 0),
                InsertAt::After => (head + grapheme_len(&text, head), String::new(), 0),
                InsertAt::FirstNonBlank => {
                    let blanks = indent_len(&text, row);
                    (start + blanks, String::new(), 0)
                }
                InsertAt::LineEnd => (end, String::new(), 0),
                InsertAt::LineBelow => {
                    let opened = format!("\n{}", spaces_of_indent(&text, row, tab));
                    let len = opened.len();
                    (end, opened, len)
                }
                InsertAt::LineAbove => {
                    let indent = spaces_of_indent(&text, row, tab);
                    let len = indent.len();
                    (start, indent + "\n", len)
                }
            }
        })
        .collect();
    // Typing begins at the insertion point; `o` and `O` begin at the caret.
    let opens = matches!(args.at, InsertAt::LineBelow | InsertAt::LineAbove);
    let begins: Vec<usize> = match opens {
        true => heads,
        false => points.iter().map(|(at, _, _)| *at).collect(),
    };
    start(model, state, origin.clone(), &text, begins);
    points.sort_by_key(|(at, _, _)| *at);
    points.dedup_by_key(|(at, _, _)| *at);
    let opened = apply_opening(
        model,
        points
            .iter()
            .filter(|(_, opened, _)| !opened.is_empty())
            .map(|(at, opened, _)| (*at..*at, opened.clone()))
            .collect(),
        origin.clone(),
    );
    // Each caret, and for `o` and `O` the start of the indentation it
    // opened, in the text right after the edit.
    let mut shift = 0;
    let carets = points
        .iter()
        .map(|(at, opened, land)| {
            let caret_at = at + shift + land;
            shift += opened.len();
            let indent = opens.then(|| caret_at - opened.trim_matches('\n').len());
            (caret_at, indent)
        })
        .collect();
    let opening = Opening {
        origin,
        count: args.count.unwrap_or(1).max(1),
        change: Change::Insert(args),
        opens,
        resumed: false,
    };
    enter(model, state, opening, carets, opened);
    Ok(Dispatched::new(Effect::Changed))
}

/// Applies a session's opening `edits`, returning the snapshot right after
/// them and a subscription from there: what measuring text edits once the
/// model catches up (see [`apply`]) is the session's to follow.
fn apply_opening(
    model: &mut EditorModel,
    edits: Vec<(Range<usize>, String)>,
    origin: Origin,
) -> (BufferSnapshot, Subscription) {
    let mut buffer = model.buffer_mut();
    buffer.edit(edits, origin);
    (buffer.snapshot(), buffer.subscribe())
}

/// `cw` and `cW` on a non-blank: from the caret's grapheme to the end of
/// its word, then on to the ends of `count - 1` more words, inclusive
/// (`:help cw`). `None` on a blank or an empty row, where they act as `dw`
/// and `dW` do.
pub(crate) fn change_word(text: &Text, head: usize, motion: Motion, count: u32) -> Option<Span> {
    let big = motion == Motion::BigWordForward;
    let mut cursor = Cursor::at(text, head);
    if cursor.line.len() == 0 || cursor.class(big) == crate::motion::Class::Blank {
        return None;
    }
    let start = cursor.offset();
    let class = cursor.class(big);
    while cursor.class_on_row(cursor.ix + 1, big) == Some(class) {
        cursor.ix += 1;
    }
    cursor.words(Motion::WordEnd, count.max(1) - 1, big);
    let end = cursor.offset();
    Some(Span::Chars(start..end + grapheme_len(text, end)))
}

/// The change operator `c` over each caret's span: deletes like `d` (the
/// registers included; rows keep the first's indentation), then starts a
/// session in the same transaction. A span that failed leaves its caret
/// where it is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn change(
    model: &mut EditorModel,
    host: &mut dyn EditorHost,
    state: &mut VimState,
    registers: &mut Registers,
    actor: &Actor,
    args: OperateArgs,
    before: &[ResolvedSelection],
    spans: &[Span],
) -> Result<Dispatched<TransactionId>, CommandError> {
    let origin = origin(actor);
    let text = model.text().clone();
    // Each caret's deletion, where its caret goes, and what the registers
    // would take.
    let mut deletes: Vec<Range<usize>> = Vec::new();
    let mut carets: Vec<(usize, Option<usize>)> = Vec::with_capacity(spans.len());
    let mut begins = Vec::with_capacity(spans.len());
    let mut write = None;
    for (ix, (selection, span)) in before.iter().zip(spans).enumerate() {
        // The caret's grapheme, wherever inside it another origin's edit
        // left the caret.
        let head = Cursor::at(&text, selection.head()).offset();
        // The registers take the primary (last) caret's text only.
        let primary = ix + 1 == spans.len();
        let register = |span: &Span| {
            primary.then(|| {
                Op::new(&text, span, false)
                    .expect("a span with text")
                    .register(&text)
            })
        };
        let mut indent = None;
        let (delete, at, value) = match span {
            Span::Chars(range) if !range.is_empty() => {
                (Some(range.clone()), range.start, register(span))
            }
            // Nothing to take, which an inclusive target (`C` or `ciw` on an
            // empty row) still writes, as nvim does.
            Span::Chars(range) => {
                let inclusive = match args.target {
                    Target::Motion(motion) => motion.class() == RangeClass::Inclusive,
                    _ => true,
                };
                (
                    None,
                    range.start,
                    (inclusive && primary).then(Register::default),
                )
            }
            Span::Lines { rows, .. } => {
                let (first, last) = (*rows.start(), *rows.end());
                let keep = text.row_start(first) + indent_len(&text, first);
                indent = Some(text.row_start(first));
                (Some(keep..row_end(&text, last)), keep, register(span))
            }
            Span::Move(offset) => (None, *offset, None),
            Span::Failed => (None, head, None),
        };
        // Where nvim's undo puts the caret back: where typing starts, but on
        // one row its first non-blank (or the caret, if it's before), and
        // over several rows the second at the caret's column.
        begins.push(match span {
            Span::Lines { rows, .. } if rows.start() == rows.end() => {
                let mut cursor = Cursor::at(&text, text.row_start(*rows.start()));
                cursor.ix = cursor.line.first_non_blank();
                history::pair(&text, cursor.offset().min(head))
            }
            Span::Lines { rows, .. } => {
                let (_, column) = history::pair(&text, head);
                history::caret_on_row(&text, rows.start() + 1, column)
            }
            _ => history::pair(&text, at),
        });
        deletes.extend(delete);
        carets.push((at, indent));
        if primary {
            write = value;
        }
    }
    if let Some(value) = write {
        let small = !value.linewise && !value.text.contains('\n');
        let percent = args.target == Target::Motion(Motion::MatchPair);
        let write = Write::Delete {
            small,
            numbered: !small || percent,
        };
        registers
            .write(args.register, write, value, host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }
    let id = model.buffer_mut().start_transaction(origin.clone());
    state.carets.record_pending(id, begins);
    let removed = merge(deletes);
    let opened = apply_opening(
        model,
        removed
            .iter()
            .map(|range| (range.clone(), String::new()))
            .collect(),
        origin.clone(),
    );
    // Each caret and its indentation, in the text right after the deletion.
    let offsets: Vec<usize> = carets
        .iter()
        .flat_map(|&(at, indent)| std::iter::once(at).chain(indent))
        .collect();
    let mut mapped = through_deletes(&offsets, &removed).into_iter();
    let mut carets: Vec<(usize, Option<usize>)> = carets
        .iter()
        .map(|&(_, indent)| {
            let at = mapped.next().expect("one per caret");
            (at, indent.and_then(|_| mapped.next()))
        })
        .collect();
    carets.sort_by_key(|(head, _)| *head);
    let opening = Opening {
        origin,
        count: 1,
        change: Change::Operate(args),
        opens: false,
        resumed: false,
    };
    enter(model, state, opening, carets, opened);
    Ok(Dispatched::new(Effect::Changed))
}

/// Types `text` at every caret, as `.` replays a session, leaving each
/// caret after it.
pub(crate) fn type_at_carets(model: &mut EditorModel, text: &str, origin: Origin) {
    model.sync_display();
    let heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    let edits = heads.iter().map(|&head| (head..head, text));
    let (_, snapshot) = apply(model, edits, origin);
    let after: Vec<usize> = heads
        .iter()
        .enumerate()
        .map(|(ix, head)| head + (ix + 1) * text.len())
        .collect();
    place(model, &snapshot, &after);
}

/// A region as the session ends.
struct Ended {
    /// Where its typed text starts.
    start: usize,
    /// The caret inside it, if one is.
    head: Option<usize>,
    /// What was typed there, up to that caret.
    typed: String,
}

/// `vim.normal_mode`: ends the session. At each caret the typed text goes
/// in `count - 1` more times (each in a new row for `o` and `O`); the
/// indentation the session made that went unused goes ([`Indents`]); the
/// transaction ends; each caret steps back one grapheme unless it's at its
/// row's start; and the mode is normal again. `.` keeps the text from the
/// primary caret's start to that caret, or nothing if it's before the start.
///
/// The session's record is locked only between the model's catch-ups (see
/// [`Ledger`]), and notes meanwhile that the session is ending, so it ends
/// once.
pub(crate) fn normal_mode(
    target: &mut VimTarget<'_>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    let ended = end(model, state, Ending::Esc);
    Ok(Dispatched::new(Effect::Changed).with_transactions(ended.into_iter().collect()))
}

/// How [`end`] ends a session.
#[derive(Copy, Clone, Debug)]
enum Ending {
    /// `Esc`: the count's copies go in, and each caret steps back.
    Esc,
    /// A split before a command that edits: typing stopped at the carets.
    Split,
    /// A split after a command that moved the carets without editing:
    /// typing stopped at this, the primary caret before it, and the
    /// indentation on the rows the carets are on now stays.
    Moved(Anchor),
}

/// Ends the open session, if any, leaving normal mode: as `Esc` does
/// ([`normal_mode`]), or for a split ([`VimState::split_session`]) without
/// the count's copies or the step back. Returns the transaction it closed,
/// if that was kept.
fn end(model: &mut EditorModel, state: &mut VimState, how: Ending) -> Option<TransactionId> {
    let esc = matches!(how, Ending::Esc);
    state.mode = Mode::Normal;
    state.goals = None;
    let session = state.session.take()?;
    let replaying = state.replaying;
    sync(model);
    let text = model.text().clone();
    let primary_start = session.regions.last().map(|region| region.start);
    let typed_first = {
        let mut ledger = session.ledger();
        ledger.follow(&session.origin);
        match std::mem::replace(&mut ledger.end, End::Ending) {
            End::Open => Ok(primary_start.is_some_and(|start| {
                let start = model.snapshot().resolve(&start);
                ledger.indents.used_before(&text, start)
            })),
            end => {
                let closed = match &end {
                    End::Ended(closed) => Some(closed.clone()),
                    _ => None,
                };
                ledger.end = end;
                Err(closed)
            }
        }
    };
    let typed_first = match typed_first {
        Ok(typed_first) => typed_first,
        // A clone of this state ended it already: this state takes the
        // outcome. One ending it now (from inside measuring text) has none
        // to give yet. Either way, its model's carets are made normal mode's.
        Err(closed) => {
            match closed {
                Some(closed) => {
                    state.last_change = Some(closed.last_change);
                    if !replaying {
                        state.carets.close(closed.transaction);
                    }
                }
                None => {
                    state.closing = Some(Closing {
                        ledger: Arc::clone(&session.ledger),
                        replaying,
                    })
                }
            }
            if esc {
                let carets: Vec<ResolvedSelection> = model
                    .selections()
                    .iter()
                    .map(|selection| caret(Cursor::at(&text, selection.head()).offset()))
                    .collect();
                model.set_selections(carets);
            }
            return None;
        }
    };
    let heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    let snapshot = model.snapshot();
    let primary = match how {
        Ending::Moved(primary) => snapshot.resolve(&primary),
        _ => model.primary_selection().head(),
    };
    let ended: Vec<Ended> = session
        .regions
        .iter()
        .map(|region| {
            let (start, end) = (
                snapshot.resolve(&region.start),
                snapshot.resolve(&region.end),
            );
            let below = heads.partition_point(|&head| head <= end);
            let head = below
                .checked_sub(1)
                .map(|ix| heads[ix])
                .filter(|&head| head >= start);
            let typed = head.map_or_else(String::new, |head| text.slice_to_string(start..head));
            Ended { start, head, typed }
        })
        .collect();
    let kept = primary_start
        .filter(|start| primary >= snapshot.resolve(start))
        .map(|start| (start, snapshot.anchor_before(primary)));

    if esc {
        repeat_typed(model, &session, &ended);
    }
    drop_unused_indent(model, &session, matches!(how, Ending::Moved(_)));

    let snapshot = model.snapshot();
    let typed = kept.map_or_else(String::new, |(start, end)| {
        let (start, end) = (snapshot.resolve(&start), snapshot.resolve(&end));
        model.text().slice_to_string(start..end.max(start))
    });
    let ended = model.buffer_mut().end_transaction();
    // An empty transaction isn't kept, so neither are its carets; inside
    // `.`, the transaction is `.`'s, which closes it.
    if !replaying {
        state.carets.close(ended);
    }

    if esc {
        let text = model.text().clone();
        let carets: Vec<ResolvedSelection> = model
            .selections()
            .iter()
            .map(|selection| {
                let head = selection.head();
                let mut cursor = Cursor::at(&text, head);
                // At a row's end, the clamp is the step back.
                if cursor.offset() == head && cursor.ix > 0 {
                    cursor.ix -= 1;
                }
                caret(cursor.offset())
            })
            .collect();
        model.set_selections(carets);
    }
    // A session a split resumed that changed nothing leaves `.` the change
    // before the split, as nvim does.
    let last_change = match &state.last_change {
        Some(last) if session.resumed && ended.is_none() => last.clone(),
        _ => LastChange {
            change: session.change.clone(),
            inserted: Some(typed),
            typed_first,
        },
    };
    session.ledger().end = End::Ended(Closed {
        last_change: last_change.clone(),
        transaction: ended,
    });
    state.last_change = Some(last_change);
    ended
}

/// A count's copies at one caret, for [`repeat_typed`].
struct Put {
    /// The caret whose typed text they copy.
    head: usize,
    /// Where they go.
    at: usize,
    text: String,
    /// What they are, for the session: their start is set once they're in.
    copies: Copies,
}

/// Puts each region's typed text in `count - 1` more times at its caret,
/// leaving the caret after the copies: at the caret, or for `o` and `O` in
/// new rows below its row. The copies hold the session's indentation as the
/// typed text does, but for what `Esc` would remove from them, and type on
/// the row each one starts on as typing them would; the rows `o` and `O`
/// open get indentation unless it would go unused. The session is told
/// what they are before they go in, as measuring text while the model
/// catches up may follow their batch (see [`Ledger`]).
fn repeat_typed(model: &mut EditorModel, session: &Session, ended: &[Ended]) {
    let more = session.count as usize - 1;
    if more == 0 {
        return;
    }
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let carets = ended.len().max(1);
    let mut puts: Vec<Put> = {
        let ledger = session.ledger();
        ended
            .iter()
            .filter_map(|region| {
                let head = region.head?;
                let typed = region.typed.as_str();
                let indents = ledger.indents.removable_in(region.start..head);
                match session.opens {
                    true => {
                        // Unused indentation goes at `Esc` from a row
                        // holding only blanks.
                        let row = row(&text, head);
                        let first = typed.split('\n').next().unwrap_or_default();
                        let rows = Rows {
                            indented: ledger.indents.used_before(&text, region.start)
                                || first.bytes().any(|byte| byte != b' ' && byte != b'\t'),
                            width: blank_width(&text.row_to_string(row), 0, tab),
                            tab,
                        };
                        let copy = copy_of(typed, &indents, true);
                        Some(Put {
                            head,
                            at: row_end(&text, row),
                            text: reopened(typed, &copy.each, rows, more, carets),
                            copies: Copies {
                                start: 0,
                                types: false,
                                indents: Vec::new(),
                            },
                        })
                    }
                    false => {
                        let count = match typed.len() {
                            0 => 0,
                            len => more.min(MAX_PUT / (len * carets)),
                        };
                        if count == 0 {
                            return None;
                        }
                        let copy = copy_of(typed, &indents, false);
                        let mut copies = copy.each.repeat(count - 1);
                        let before = copies.len();
                        copies.push_str(&copy.last);
                        Some(Put {
                            head,
                            at: head,
                            text: copies,
                            copies: Copies {
                                start: 0,
                                types: !typed.starts_with('\n'),
                                indents: copy
                                    .open
                                    .iter()
                                    .map(|indent| indent.shifted(before))
                                    .collect(),
                            },
                        })
                    }
                }
            })
            .collect()
    };
    puts.sort_by_key(|put| put.at);
    puts.dedup_by_key(|put| put.at);
    // `o` and `O` may have had no room left for a row.
    puts.retain(|put| !put.text.is_empty());
    if puts.is_empty() {
        return;
    }
    let heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    // Where each put starts once all are in, in the text right after them.
    let mut shift = 0;
    for put in &mut puts {
        put.copies.start = put.at + shift;
        shift += put.text.len();
    }
    // A caret that put copies goes after them; the rest move with the text
    // (puts and carets are both in order).
    let after: HashMap<usize, usize> = puts
        .iter()
        .map(|put| (put.head, put.copies.start + put.text.len()))
        .collect();
    let (mut next, mut shift) = (0, 0);
    let carets: Vec<usize> = heads
        .iter()
        .map(|&head| {
            while let Some(put) = puts.get(next).filter(|put| put.at < head) {
                shift += put.text.len();
                next += 1;
            }
            after.get(&head).copied().unwrap_or(head + shift)
        })
        .collect();
    let (edits, copies): (Vec<_>, Vec<_>) = puts
        .into_iter()
        .map(|put| ((put.at..put.at, put.text), put.copies))
        .unzip();
    // Their batch is the next, which any insertion makes.
    let copied = Copied {
        from: model.buffer().version(),
        copies,
    };
    #[cfg(test)]
    COPIES_NOTED.set(copied.entries());
    session.ledger().copies = Some(copied);
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    place(model, &snapshot, &carets);
}

/// A copy of `typed` for a count: at the caret, or with `opens` in a row of
/// its own (`o` and `O`). It leaves out the pieces of `indents` (from
/// `typed`'s start) that `Esc` would remove from it, which happens to a
/// piece on a row holding only blanks that nothing was typed on
/// ([`Indents::unused`]), wherever the copies alone decide that.
///
/// A copy's rows are `typed`'s, after the blanks `o` and `O` open each with.
/// At the caret, though, a copy's first row is typed on (the copy types
/// its first row there), and its last row runs on into the next copy's
/// first, which types on it unless `typed`'s first row is empty. The last
/// copy's last row runs on into the text after the copies, so the pieces
/// there stay in, returned as `open`, for `Esc` to decide.
fn copy_of(typed: &str, indents: &[Indent], opens: bool) -> Copy {
    let rows = typed.split('\n').count();
    let types_first = typed
        .split('\n')
        .next()
        .is_some_and(|first| !first.is_empty());
    let mut copy = Copy {
        each: String::with_capacity(typed.len()),
        last: String::with_capacity(typed.len()),
        open: Vec::new(),
    };
    let (mut at, mut next) = (0, 0);
    for (row, line) in typed.split('\n').enumerate() {
        let end = at + line.len();
        let on_row = indents[next..].partition_point(|indent| indent.range.end <= end);
        let pieces = &indents[next..next + on_row];
        next += on_row;
        let blank = line.chars().all(is_blank);
        // Whether `Esc` would remove this row's pieces from a copy (the last
        // one if `last`), or `None` if the copies don't decide it.
        let removed = |last: bool| match row {
            _ if opens => Some(blank),
            0 => Some(false),
            _ if row + 1 < rows => Some(blank),
            _ if last => None,
            _ => Some(blank && !types_first),
        };
        push_row(
            &mut copy.each,
            typed,
            at..end,
            pieces,
            removed(false) == Some(true),
        );
        if removed(true).is_none() {
            let shift = copy.last.len();
            copy.open.extend(pieces.iter().map(|piece| {
                Indent::new(piece.range.start - at + shift..piece.range.end - at + shift)
            }));
        }
        push_row(
            &mut copy.last,
            typed,
            at..end,
            pieces,
            removed(true) == Some(true),
        );
        if row + 1 < rows {
            copy.each.push('\n');
            copy.last.push('\n');
        }
        at = end + 1;
    }
    copy
}

/// Pushes `typed[range]` onto `out`, without the bytes of `pieces` (in it,
/// in order) if `strip`.
fn push_row(out: &mut String, typed: &str, range: Range<usize>, pieces: &[Indent], strip: bool) {
    let mut from = range.start;
    if strip {
        for piece in pieces {
            out.push_str(&typed[from..piece.range.start]);
            from = piece.range.end;
        }
    }
    out.push_str(&typed[from..range.end]);
}

/// What `o` or `O` with a count adds below the row its session ended on:
/// up to `more` rows, each holding `each` (the copy of `typed`) after the
/// indentation `rows` gives it, as many as fit in [`MAX_PUT`] bytes over
/// `carets` carets, counted as typing `typed` again would put them in.
fn reopened(typed: &str, each: &str, mut rows: Rows, more: usize, carets: usize) -> String {
    let last = typed
        .rsplit_once('\n')
        .map(|(_, last)| blank_width(last, 0, rows.tab));
    let (mut copies, mut size) = (String::new(), 0usize);
    for _ in 0..more {
        let indent = rows.next(typed, last);
        size += 1 + indent + typed.len();
        if size.saturating_mul(carets) > MAX_PUT {
            break;
        }
        copies.push('\n');
        copies.extend(std::iter::repeat_n(' ', indent));
        copies.push_str(each);
    }
    copies
}

impl Rows {
    /// The width of the next row's indentation, which holds `typed`; `last`
    /// is the width of `typed`'s last row's indentation if it has more than
    /// one.
    fn next(&mut self, typed: &str, last: Option<usize>) -> usize {
        let indent = if self.indented { self.width } else { 0 };
        // The row it ends on is `typed`'s last, after the indentation if
        // `typed` is one row.
        self.width = last.unwrap_or_else(|| blank_width(typed, indent, self.tab));
        indent
    }
}

/// The column the blanks `line` starts with reach from `column`, a tab
/// going on to the next stop.
fn blank_width(line: &str, column: usize, tab: usize) -> usize {
    line.bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .fold(column, |column, byte| match byte {
            b'\t' => column + tab - column % tab,
            _ => column + 1,
        })
}

impl Copied {
    /// Adds what the copies leave to the session to `indents`, which their
    /// batch left `text`: the indentation left in them that `Esc` may
    /// remove, and the row the first at each caret types on.
    fn note(self, indents: &mut Indents, text: &Text) {
        indents.add(self.copies.iter().flat_map(|copies| {
            let start = copies.start;
            copies
                .indents
                .iter()
                .map(move |indent| indent.shifted(start))
        }));
        for copies in self.copies.iter().filter(|copies| copies.types) {
            indents.typed_on_row(text, copies.start);
        }
    }

    /// How many entries it holds: one per caret, and the indentation left
    /// in each caret's copies, whatever the count.
    #[cfg(test)]
    fn entries(&self) -> usize {
        self.copies
            .iter()
            .map(|copies| 1 + copies.indents.len())
            .sum()
    }
}

/// Removes the indentation the session made that went unused, as Vim's
/// 'autoindent' does on `Esc`: see [`Indents::unused`]. With `off_caret_rows`,
/// only from rows no caret is on, as nvim does when the cursor leaves a row
/// in insert mode. The selections move with the text, and are left alone
/// (their goal columns too) if nothing before them went.
fn drop_unused_indent(model: &mut EditorModel, session: &Session, off_caret_rows: bool) {
    sync(model);
    let mut removed = {
        let mut ledger = session.ledger();
        ledger.follow(&session.origin);
        ledger.indents.unused(model.text())
    };
    let selections = model.selections();
    if off_caret_rows {
        let text = model.text();
        let rows: Vec<u32> = selections.iter().map(|s| row(text, s.head())).collect();
        removed.retain(|range| !rows.contains(&row(text, range.start)));
    }
    if removed.is_empty() {
        return;
    }
    let ends: Vec<usize> = selections
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .collect();
    let mapped = through_deletes(&ends, &removed);
    let edits = removed.iter().map(|range| (range.clone(), ""));
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    if mapped == ends {
        return;
    }
    let anchors: Vec<Anchor> = mapped
        .iter()
        .map(|&offset| snapshot.anchor_before(offset))
        .collect();
    let mapped = model.snapshot().resolve_many(&anchors);
    let target: Vec<ResolvedSelection> = selections
        .iter()
        .zip(mapped.chunks(2))
        .map(|(s, ends)| ResolvedSelection {
            range: ends[0]..ends[1].max(ends[0]),
            reversed: s.reversed,
        })
        .collect();
    // The model's selections are anchored, so they follow the deletions
    // themselves; setting them would clear the vertical goal columns
    // (`o<Down><Down>` keeps its column), so they're set only where the
    // anchors landed elsewhere.
    sync(model);
    if model.selections() != target {
        model.set_selections(target);
    }
}
