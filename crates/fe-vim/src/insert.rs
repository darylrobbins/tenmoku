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
//!
//! The rows `o` and `O` open (each of a count's copies too) and a linewise
//! `c` empties are indented as the host suggests for the buffer's language
//! (`EditorHost::suggest_indent`), as nvim's are by a filetype's
//! 'indentexpr', and `Enter` through [`VimState::line_break_in`] indents
//! as the editor's line break does; with no suggestion, as with no
//! language, they copy indentation as Vim's 'autoindent' does. Either way
//! it's the session's indentation, which `Esc` drops if unused. There's no
//! `=` operator yet to re-indent rows already there.

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
    EditorModel, IndentStyle, IndentSuggestion,
    commands::{EditorHost, origin},
};
use fe_text::Text;

use crate::{
    Change, InsertArgs, InsertAt, LastChange, Mode, Motion, OperateArgs, RangeClass, Register,
    Registers, Target, VimState, VimTarget, Write, history, marks,
    motion::Cursor,
    operator::{
        MAX_PUT, Op, Span, apply, apply_planned, caret, grapheme_len, merge, row_end, sync,
    },
    visual,
};

mod block_rows;
use block_rows::{Fate, Watch};

#[cfg(test)]
mod indent_tests;

/// An open insert session.
#[derive(Clone, Debug)]
pub(crate) struct Session {
    /// One per caret, in order: the last is the primary caret's.
    regions: Vec<Region>,
    /// Shared by clones of the state, so they agree.
    ledger: Arc<Mutex<Ledger>>,
    pub(crate) origin: Origin,
    /// How many times the typed text goes in.
    count: u32,
    /// The change that opened it, kept for `.` with the typed text.
    pub(crate) change: Change,
    /// `o` and `O`: a count opens more rows.
    opens: bool,
    /// It continues a session a split ended ([`VimState::split_session`]).
    resumed: bool,
    /// Over a block, the block, which the sessions splits resume keep.
    block: Option<Block>,
}

/// A block's rows below its first, where `Esc` types a session's text too
/// (`:help v_b_I`, `v_b_A` and `v_b_c`): what its first row grew by, taken
/// from where typing began on it, as nvim takes it ([`FirstRow`]).
#[derive(Clone, Debug)]
pub(crate) struct BlockRows {
    /// Those reaching the block's column, or padded to it (`A`).
    pub(crate) rows: Vec<BlockRow>,
    /// What the text takes before it: on the first row, `A` went after a tab
    /// in the block straddling the column, which nvim types with the text.
    pub(crate) prefix: String,
    /// Where `I` and `A` leave the caret once text went in: this display
    /// column on the first row, or where typing began left of it (for `I`,
    /// right of it too). `c` steps back as `Esc` does.
    pub(crate) corner: Option<usize>,
    /// The text the opening edits left, which the rows are placed in.
    pub(crate) text: Text,
    pub(crate) edge: Edge,
    pub(crate) tab: usize,
}

/// Where `Esc` takes the text it types on a block's other rows from, and
/// where it types it there, once typing began elsewhere on the first row
/// (after a split: see [`VimState::split_session`]), as nvim does.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Edge {
    /// `c`: at the block's left column.
    Fixed,
    /// `I`: at the block's left column, or at the leftmost typing began
    /// right of it, if none began at it.
    Left,
    /// `A`: past the block's right edge, or where typing began at or left
    /// of the block's left column, as `I` types there.
    Right,
    /// `$A`: at each row's end, or as far left of the end of the block's
    /// widest row, this wide, as typing began left of the first row's end
    /// (a row short of that at its end); nowhere if that's further.
    End(usize),
}

/// A block a session types on, and its first row as followed since the
/// block's first session opened.
#[derive(Clone, Debug)]
struct Block {
    rows: BlockRows,
    /// Shared by clones of the state, and the sessions splits resume.
    first: Arc<Mutex<FirstRow>>,
    /// Its other rows, followed by who edited them.
    watch: Arc<Mutex<Watch>>,
}

/// A block's first row, followed through every edit from the opening of
/// the block's first session to its `Esc`: what the session's own edits
/// grew it by (the row's length, as nvim measures it, so text after a line
/// break typed on it isn't) is what `Esc` takes, from where typing began on
/// it, as nvim does after the cursor keys split the session. Another
/// origin's edit on it before the end of what's taken leaves where that is
/// unknown, and so does the session's own edit taking out the line break
/// ending it, unless the session typed that line break (joining the row
/// below to it): `Esc` then types nothing on the other rows. Rows put in
/// above it or taken out don't count.
struct FirstRow {
    events: Subscription,
    /// The text as of the last batch followed, and where the row starts in
    /// it.
    text: Text,
    start: usize,
    /// The session typed the line break ending it now.
    typed_ending: bool,
    /// Where typing began on it when the block's session opened, from its
    /// start.
    column: usize,
    /// How much the session's own edits grew its length.
    grown: isize,
    /// Where the open session began, from its start, if on it; the leftmost
    /// a session that edited began at.
    began: Option<usize>,
    leftmost: Option<usize>,
    /// The leftmost another origin edited it at, followed since.
    other: Option<usize>,
    /// The session's own edit took out a line break ending it that it
    /// didn't type.
    joined: bool,
}

impl fmt::Debug for FirstRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FirstRow")
            .field("start", &self.start)
            .field("grown", &self.grown)
            .finish_non_exhaustive()
    }
}

/// What `Esc` types on a block's other rows.
struct Typed {
    /// What goes before each count's copies of `text`.
    prefix: String,
    text: String,
    /// The display column it goes at there, if not the block's.
    column: Option<usize>,
    /// The display column `I` and `A` leave the caret at on the first row.
    corner: Option<usize>,
}

/// Where `Esc` types on one of [`BlockRows`], placed when the session opened
/// as the block's column fell on the row (its `text`, in [`BlockRows::text`]):
/// `place`, offsets from the row's start (a tab it splits, or the point the
/// text goes in at), becomes `spaces.0` spaces with the text after
/// `spaces.1` of them. `Esc` finds the row again by anchors at its `ends`,
/// past text put in at either, and at the line break `ending` it, if one
/// does: they follow rows put in or taken out around it, and an edit undone
/// at once. One that's no longer as it was gets none, where the block's
/// column falls there not being known any more: its text changed by another
/// origin, joined to another row, or gone with its line break.
#[derive(Clone, Debug)]
pub(crate) struct BlockRow {
    pub(crate) ends: Range<Anchor>,
    pub(crate) ending: Option<Range<Anchor>>,
    pub(crate) text: Range<usize>,
    pub(crate) place: Range<usize>,
    pub(crate) spaces: (usize, usize),
}

impl BlockRow {
    /// Its anchors, in the order [`BlockRow::found`] takes them.
    fn anchors(&self) -> impl Iterator<Item = Anchor> + '_ {
        let ending = self
            .ending
            .iter()
            .flat_map(|ending| [ending.start, ending.end]);
        [self.ends.start, self.ends.end].into_iter().chain(ending)
    }

    /// Where the row starts in `text` if it's as it was, given where its
    /// anchors are now, in order: still holding its text (`was`) between its
    /// ends, or for an empty row, still one where the anchor at its start
    /// is, which keeps just before the line break that ended it (the last
    /// row's: the end of the text); and still ended by its line break.
    fn found(
        &self,
        (text, was): (&Text, &Text),
        anchors: &mut impl Iterator<Item = usize>,
    ) -> Option<usize> {
        let mut next = || anchors.next().expect("one offset per anchor");
        let (start, end) = (next(), next());
        let ending = self.ending.as_ref().map(|_| (next(), next()));
        if ending.is_some_and(|(start, end)| start >= end) {
            return None;
        }
        if self.text.is_empty() {
            let point = text.offset_to_point(start);
            return (point.column == 0 && row_end(text, point.row) == start).then_some(start);
        }
        let point = text.offset_to_point(start);
        let same = start <= end
            && point.column == 0
            && row_end(text, point.row) == end
            && text.range_eq(start..end, &was.slice_to_string(self.text.clone()));
        same.then_some(start)
    }
}

impl Block {
    fn first(&self) -> MutexGuard<'_, FirstRow> {
        self.first.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn watch(&self) -> MutexGuard<'_, Watch> {
        self.watch.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What `Esc` types on the other rows, with the primary caret at `head`
    /// in `first`'s text: what the first row grew by, taken from where
    /// typing began (see [`Edge`]), with the prefix if none began left of
    /// that, if what's taken is known and, for `I` and `A`, the caret is on
    /// the row (`c` takes it wherever the caret is, as nvim does).
    fn typed(&self, first: &FirstRow, head: usize) -> Option<Typed> {
        let text = &first.text;
        let point = text.offset_to_point(first.start);
        let grown = usize::try_from(first.grown).ok()?;
        let on_row = text.offset_to_point(head).row == point.row;
        if first.joined || point.column != 0 || !(on_row || matches!(self.rows.edge, Edge::Fixed)) {
            return None;
        }
        let line = text.slice_to_string(first.start..row_end(text, point.row));
        let tab = self.rows.tab;
        let columns = |at: usize| visual::width_from(&line[..line.floor_char_boundary(at)], 0, tab);
        let began = first.leftmost.unwrap_or(first.column);
        let at = columns(began);
        let (from, place) = match self.rows.edge {
            Edge::Left if began > first.column => (began, Some(at)),
            Edge::Right if self.rows.corner.is_some_and(|left| at <= left) => (began, Some(at)),
            Edge::End(width) if began < first.column => {
                let column = width
                    .checked_sub(first.column - began)
                    .filter(|&at| at > 0)?;
                (began, Some(column))
            }
            _ => (first.column, None),
        };
        // From the start of a character a fixed column falls inside, as
        // nvim takes it.
        let to = from + grown;
        if to > line.len() || first.other.is_some_and(|other| other < first.start + to) {
            return None;
        }
        let taken = &line[line.floor_char_boundary(from)..line.floor_char_boundary(to)];
        let prefix = match began < first.column {
            true => "",
            false => self.rows.prefix.as_str(),
        };
        if taken.is_empty() && prefix.is_empty() {
            return None;
        }
        let corner = self.rows.corner.map(|left| match self.rows.edge {
            Edge::Left => at,
            _ => left.min(at),
        });
        Some(Typed {
            prefix: prefix.to_string(),
            text: taken.to_string(),
            column: place,
            corner,
        })
    }
}

impl FirstRow {
    /// Follows the buffer's edits since the last call: `origin`'s are the
    /// session's (but for its undos and redos).
    fn follow(&mut self, origin: &Origin) {
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
            let old = std::mem::replace(&mut self.text, text);
            let new = &self.text;
            let start = self.start;
            let end = row_end(&old, row(&old, start));
            // Whole rows another origin put in or took out right above it.
            let line_break = |text: &Text, at: usize| text.bytes_at(at).first() == Some(&b'\n');
            let above = |edit: &Edit| {
                !own && edit.old.end == start
                    && !(edit.old.is_empty() && edit.new.is_empty())
                    && (edit.new.is_empty() || line_break(new, edit.new.end - 1))
                    && (edit.old.is_empty()
                        || edit.old.start == 0
                        || line_break(&old, edit.old.start - 1))
            };
            self.other = self.other.map(|other| point(&edits, other, true));
            let mut past = false;
            for edit in edits.iter() {
                if edit.old.start > end || edit.old.end < start {
                    continue;
                }
                if above(edit) {
                    past |= edit.old.is_empty();
                } else if !own {
                    let at = edit.new.start;
                    self.other = Some(self.other.map_or(at, |other| other.min(at)));
                }
            }
            // The line break ending it, taken out, or put in by this batch.
            let ended = end < old.len()
                && edits
                    .iter()
                    .any(|edit| edit.old.start <= end && end < edit.old.end);
            self.joined |= own && ended && !self.typed_ending;
            self.start = point(&edits, start, past);
            let new_end = row_end(new, row(new, self.start));
            if edits
                .iter()
                .any(|edit| edit.new.start <= new_end && new_end < edit.new.end)
            {
                self.typed_ending = own;
            } else if ended {
                self.typed_ending = false;
            }
            if own {
                self.grown += (new_end - self.start) as isize - (end - start) as isize;
            }
        }
    }

    /// Notes that a session begins at `head`, an offset in the text as of
    /// the last batch followed.
    fn begin(&mut self, head: usize) {
        let end = row_end(&self.text, row(&self.text, self.start));
        self.began = (self.start..=end)
            .contains(&head)
            .then(|| head - self.start);
    }

    /// Notes that the open session ends, having `edited` the text or not:
    /// where it began counts then, if leftmost (as nvim's start of insert).
    fn end(&mut self, edited: bool) {
        if let Some(at) = self.began.take().filter(|_| edited) {
            self.leftmost = Some(self.leftmost.map_or(at, |leftmost| leftmost.min(at)));
        }
    }
}

/// What an open session follows: the buffer's edits since it opened, and
/// the indentation it made, as of the last batch of them followed. It's
/// locked only between the model's catch-ups, never across one: measuring
/// text as the model catches up may stamp the context through a clone of
/// the state, which follows the buffer's edits here.
struct Ledger {
    events: Subscription,
    indents: Indents,
    /// The line breaks typed through [`VimState::line_break`], until
    /// followed.
    breaks: Vec<Break>,
    /// The count's copies `vim.normal_mode` puts in, until followed.
    copies: Option<Copied>,
    /// How far ending the session has got, through this state or a clone.
    end: End,
    /// Whether the session's origin edited the text since it opened.
    edited: bool,
    /// The versions the batches ending it made (the count's copies, the
    /// indentation `Esc` takes out, a block's other rows), until noted
    /// for the marks (see `marks::Note::Ending`).
    ending: Vec<Version>,
}

/// A line break typed in insert or replace mode, by the batches it made.
#[derive(Copy, Clone, Debug)]
enum Break {
    /// The batch from this version.
    One(Version),
    /// The batches from the first version to the second, once it's typed
    /// (`EditorModel::newline_in` may make two).
    Many(Version, Option<Version>),
}

impl Break {
    fn from(self) -> Version {
        match self {
            Break::One(from) | Break::Many(from, _) => from,
        }
    }

    /// Whether the batch from version `from` is the line break's.
    fn holds(self, from: Version) -> bool {
        match self {
            Break::One(at) => at == from,
            Break::Many(start, until) => start <= from && until.is_none_or(|until| from < until),
        }
    }

    /// Whether no batch after the one from version `from` can be its.
    fn spent(self, from: Version) -> bool {
        match self {
            Break::One(at) => at <= from,
            Break::Many(_, until) => until.is_some_and(|until| until <= from),
        }
    }
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
    /// The session's own carets record, which a newer session mustn't
    /// overwrite or close before this one is adopted.
    carets: Option<history::Pending>,
    /// `VimState::changes` when it was left: a newer change keeps `.`.
    changes: u64,
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
    let Some(Closing {
        replaying,
        carets,
        changes,
        ..
    }) = state.closing.take()
    else {
        return;
    };
    // A change made while it was closing is newer than it for `.`. The
    // marks it left go only where nothing set them since (see
    // `marks::adopt_insert`): a yank or `m[` sets some, but not `'.`.
    if state.changes == changes {
        state.set_last_change(closed.last_change);
    }
    if !replaying {
        state.carets.settle(carets, closed.transaction);
    }
    match closed.left {
        Some(left) => marks::adopt_insert(state, *left),
        None => marks::closed(state),
    }
}

/// What ending a session left: the last change for `.`, the transaction it
/// closed, if that was kept, for the carets record, and at `Esc` the marks
/// leaving insert mode set.
#[derive(Clone, Debug)]
struct Closed {
    last_change: LastChange,
    transaction: Option<TransactionId>,
    left: Option<Box<marks::Left>>,
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
    /// The last rows of `each` and of `last`.
    each_tail: Tail,
    last_tail: Tail,
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
    ///
    /// A session over a block (`c`, `I` or `A`) keeps it: `Esc` types on its
    /// other rows what its first row grew by, if the caret is on that row
    /// then (for `c`, wherever it is), as the crate docs say.
    pub fn split_session<R>(
        &mut self,
        model: &mut EditorModel,
        splits: Splits,
        command: impl FnOnce(&mut EditorModel) -> R,
    ) -> R {
        let Some((origin, block)) = self
            .session
            .as_ref()
            .map(|session| (session.origin.clone(), session.block.clone()))
        else {
            return command(model);
        };
        // A replace session resumes as one.
        let replacing = self.mode == Mode::Replace;
        if splits == Splits::Edits {
            end(model, self, Ending::Split, None);
            let result = command(model);
            resume(model, self, origin, block);
            if replacing {
                crate::replace::resumed(self, model);
            }
            return result;
        }
        sync(model);
        let before = model.selections();
        let primary = model
            .snapshot()
            .anchor_before(model.primary_selection().head());
        let result = command(model);
        if splits == Splits::Other || model.selections() != before {
            end(model, self, Ending::Moved(primary), None);
            resume(model, self, origin, block);
            if replacing {
                crate::replace::resumed(self, model);
            }
        }
        result
    }
}

/// Opens the session a split resumes ([`VimState::split_session`]): typing
/// starts at each selection's start, where typing over it goes, and the
/// selections stay. It goes on with a `block` the one before was over.
fn resume(model: &mut EditorModel, state: &mut VimState, origin: Origin, block: Option<Block>) {
    model.sync_display();
    let text = model.text().clone();
    let mut heads: Vec<usize> = model
        .selections()
        .iter()
        .map(|selection| selection.range.start)
        .collect();
    heads.sort_unstable();
    heads.dedup();
    if let (Some(block), Some(&head)) = (&block, heads.last()) {
        let mut first = block.first();
        first.follow(&origin);
        first.begin(head);
    }
    state.marks.typing(model.snapshot(), heads.iter().copied());
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
        block,
    };
    open(state, opening, &heads, Indents::default(), opened);
}

impl Session {
    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A batch ending the session made `version` (see [`Ledger::ending`]).
    pub(crate) fn ending(&self, version: Version) {
        self.ledger().ending.push(version);
    }

    /// Follows the buffer's edits since the last call, so none wait to be:
    /// see [`Ledger::follow`] and [`FirstRow::follow`].
    pub(crate) fn follow_edits(&self) {
        self.ledger().follow(&self.origin);
        if let Some(block) = &self.block {
            block.first().follow(&self.origin);
            block.watch().follow(&self.origin);
        }
    }

    /// Notes that the batch from version `from` is a line break typed in
    /// insert or replace mode: the white space it puts after the break is
    /// the new row's indentation, which `Esc` may drop. Called before the
    /// line break is typed, and holds nothing locked once it returns, so the
    /// session can follow the batch while it's typed.
    pub(crate) fn typed_line_break(&self, from: Version) {
        self.ledger().breaks.push(Break::One(from));
    }

    /// [`Session::typed_line_break`] for a line break that may take more
    /// than a batch (`EditorModel::newline_in`): every batch from `from`
    /// is its, until [`Session::line_break_typed`] says where they end.
    pub(crate) fn typing_line_break(&self, from: Version) {
        self.ledger().breaks.push(Break::Many(from, None));
    }

    /// Notes that the line break typed from version `from` left version
    /// `to`: later batches aren't its.
    pub(crate) fn line_break_typed(&self, from: Version, to: Version) {
        for line_break in &mut self.ledger().breaks {
            if let Break::Many(start, until @ None) = line_break
                && *start == from
            {
                *until = Some(to);
            }
        }
    }

    /// Takes back [`Session::typed_line_break`]'s note of `from` when the
    /// line break typed nothing (there was no caret to type at), so no later
    /// batch from that version is taken for it.
    pub(crate) fn forget_line_break(&self, from: Version) {
        self.ledger()
            .breaks
            .retain(|line_break| line_break.from() != from);
    }

    /// Notes the rows the session began on as typed on, as `.` does when
    /// the change it repeats typed there (see [`LastChange`]).
    pub(crate) fn typed_on_first_rows(&self) {
        for indent in &mut self.ledger().indents.0 {
            indent.used = true;
        }
    }

    /// Notes `offset`'s row in `text`, the model's, as typed on: a replace
    /// session's copies type there what they then take back.
    pub(crate) fn typed_on_row(&self, text: &Text, offset: usize) {
        let mut ledger = self.ledger();
        ledger.follow(&self.origin);
        ledger.indents.typed_on_row(text, offset);
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
            Ok(ledger) => Some(ledger.breaks.iter().map(|at| at.from()).collect()),
            Err(std::sync::TryLockError::Poisoned(ledger)) => Some(
                ledger
                    .into_inner()
                    .breaks
                    .iter()
                    .map(|at| at.from())
                    .collect(),
            ),
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
                self.followed_breaks(from);
                continue;
            }
            let line_break = self.breaks.iter().any(|line_break| line_break.holds(from));
            let whose = if &event_origin != origin || kind != EditKind::Edit {
                Whose::Other
            } else if line_break {
                Whose::LineBreak
            } else {
                Whose::Typing
            };
            self.edited |= whose != Whose::Other;
            self.indents.follow(&edits, &text, whose);
            self.followed_breaks(from);
        }
    }

    /// Forgets the line breaks no batch after the one from `from` can be.
    fn followed_breaks(&mut self, from: Version) {
        self.breaks.retain(|line_break| !line_break.spent(from));
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
    /// (`EditorModel::newline`, which copies the row's white space, or
    /// `newline_in`, which may then re-indent the row): the white space
    /// after the break is the new row's indentation.
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
        let (mut typed, mut pieces) = (Vec::new(), Vec::new());
        if whose == Whose::Typing {
            pieces.extend(edits.iter().filter_map(|edit| outdent(edit, text)));
        }
        for edit in edits.iter().filter(|edit| !edit.new.is_empty()) {
            let inserted = text.slice_to_string(edit.new.clone());
            let Some(rows) = inserted
                .strip_prefix('\n')
                .filter(|_| whose == Whose::LineBreak)
            else {
                typed.push((edit.new.start, inserted));
                continue;
            };
            // The rows after the break: blanks alone are indentation, and a
            // row with more is a pair's closer, split onto a row of its own
            // (`EditorModel::newline_in`).
            let mut at = edit.new.start + 1;
            for line in rows.split('\n') {
                match line.chars().all(is_blank) {
                    true if !line.is_empty() => pieces.push(Indent::new(at..at + line.len())),
                    true => {}
                    false => typed.push((at, line.to_owned())),
                }
                at += line.len() + 1;
            }
        }
        self.add(pieces);
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

    /// Where typing that began at `offset` begins now: past the pieces that
    /// start there, which a typed closer's outdent made of the blanks
    /// before it ([`outdent`]), collapsing the start onto its row's.
    fn past(&self, offset: usize) -> usize {
        let ix = self.0.partition_point(|indent| indent.range.start < offset);
        self.0[ix..]
            .iter()
            .take_while(|indent| indent.range.start == offset)
            .fold(offset, |at, indent| at.max(indent.range.end))
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

    /// How much of the indentation the session made, whether used or not,
    /// starts `range`'s last row (`range` holds `typed`), for `'.`: the
    /// count's copies of it get that as typing it again would.
    fn lead_in(&self, range: Range<usize>, typed: &str) -> usize {
        let start = range.start + typed.rfind('\n').map_or(0, |at| at + 1);
        let low = self.0.partition_point(|indent| indent.range.end <= start);
        let mut lead = start;
        for indent in self.0[low..].iter().filter(|indent| !indent.changed) {
            if indent.range.start > lead || indent.range.start >= range.end {
                break;
            }
            lead = lead.max(indent.range.end.min(range.end));
        }
        lead - start
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

/// Where `range`, in the text at version `since`, is in `snapshot`'s, if no
/// edit since wrote inside it or at its edges (as measuring text may,
/// through another model), so what was there is as it was, and runs on
/// into nothing new: blanks put in at its end would lengthen its last
/// row's.
fn untouched(
    snapshot: &BufferSnapshot,
    since: Version,
    mut range: Range<usize>,
) -> Option<Range<usize>> {
    if since < snapshot.oldest_version() {
        return None;
    }
    for batch in snapshot.edits_since(since) {
        let inside = batch
            .iter()
            .any(|edit| edit.old.start <= range.end && range.start <= edit.old.end);
        if inside {
            return None;
        }
        let before = batch.partition_point(|edit| edit.old.end <= range.start);
        if let Some(edit) = before.checked_sub(1).map(|ix| &batch[ix]) {
            let shift = |offset: usize| offset - edit.old.end + edit.new.end;
            range = shift(range.start)..shift(range.end);
        }
    }
    Some(range)
}

/// The indentation `edit`, typed, leaves, if it's the editor lining a
/// typed closer up with its opener (`EditorModel::insert_typed_in`, or
/// `.` typing it again: [`outdent_closers`]), in `text`, the text after it:
/// the row's blanks, if any, and the closer after them rewritten from the
/// row's start, which leaves blanks and that one char. The blanks are the
/// row's indentation still, not typed text, which a session's typing
/// begins past ([`Indents::past`]) wherever it began on the row.
fn outdent(edit: &Edit, text: &Text) -> Option<Indent> {
    let at_row_start = edit.new.start == text.row_start(row(text, edit.new.start));
    if edit.old.is_empty() || edit.new.is_empty() || !at_row_start {
        return None;
    }
    let new = text.slice_to_string(edit.new.clone());
    let blanks = blank_len(&new);
    let mut rest = new[blanks..].chars();
    rest.next().filter(|ch| !ch.is_whitespace())?;
    (blanks > 0 && rest.next().is_none())
        .then(|| Indent::new(edit.new.start..edit.new.start + blanks))
}

/// What's left of `range`, in the text before a batch of `edits`, in the
/// text after it: the bytes no edit replaced, in pieces split wherever an
/// edit replaced some or put text in strictly inside. An empty range is a
/// point, which stays before text put in at it and goes to the start of a
/// replacement it was inside. Also whether any edit touched its bytes so.
fn survive(edits: &[Edit], range: &Range<usize>) -> (Vec<Range<usize>>, bool) {
    if range.is_empty() {
        let mapped = point(edits, range.start, false);
        return (std::iter::once(mapped..mapped).collect(), false);
    }
    let shift = |offset: usize, passed: usize| shifted(edits, offset, passed);
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

/// Where `at`, an offset in the text before a batch of `edits`, is in the
/// text after it: past text put in at it if `past`, else before it, past a
/// replacement it ends, and at the start of one it's inside or starts.
fn point(edits: &[Edit], at: usize, past: bool) -> usize {
    let ix = edits.partition_point(|edit| edit.old.end < at || (past && edit.old.end == at));
    match edits.get(ix) {
        Some(edit) if edit.old.start < at && at < edit.old.end => edit.new.start,
        Some(edit) if edit.old.start < at => edit.new.end,
        Some(edit) if edit.old.start == at => edit.new.start,
        _ => shifted(edits, at, ix),
    }
}

/// `offset`, which lies after the first `passed` of `edits` and before the
/// rest, in the text after them all.
fn shifted(edits: &[Edit], offset: usize, passed: usize) -> usize {
    match passed.checked_sub(1) {
        Some(ix) => offset - edits[ix].old.end + edits[ix].new.end,
        None => offset,
    }
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
pub(crate) fn place(model: &mut EditorModel, snapshot: &BufferSnapshot, heads: &[usize]) {
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

/// `row`'s leading blanks in `text`, or as `planned` if there; none past
/// the text.
fn indent_of(text: &Text, row: u32, planned: &HashMap<u32, String>) -> String {
    match planned.get(&row) {
        Some(indent) => indent.clone(),
        None if row < text.row_count() => {
            let line = text.row_to_string(row);
            line[..blank_len(&line)].to_owned()
        }
        None => String::new(),
    }
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
    block: Option<Block>,
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
    let regions: Vec<Region> = heads
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
        edited: false,
        ending: Vec::new(),
    }));
    let Opening {
        origin,
        count,
        change,
        opens,
        resumed,
        block,
    } = opening;
    // Typing from here on is vim's, and leaving insert mode leaves `'[`
    // where it began.
    let primary = heads.last().copied().unwrap_or(0);
    state.marks.inserting(&snapshot, primary, origin.clone());
    state.session = Some(Session {
        regions,
        ledger,
        origin,
        count,
        change,
        opens,
        resumed,
        block,
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
    state.marks.own(id);
}

/// `vim.insert`: moves each caret as `at` says (opening a row for `o` and
/// `O`) and starts a session.
pub(crate) fn insert(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: InsertArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget {
        model, state, host, ..
    } = target;
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
    if args.at == InsertAt::LineAbove {
        for &head in &heads {
            let row = row(&text, head);
            state.marks.opening(model.snapshot(), row);
        }
    }
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
    if !opens {
        state.marks.typing(model.snapshot(), begins.iter().copied());
    }
    start(model, state, origin.clone(), &text, begins);
    points.sort_by_key(|(at, _, _)| *at);
    points.dedup_by_key(|(at, _, _)| *at);
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
    let edits = points
        .iter()
        .filter(|(_, opened, _)| !opened.is_empty())
        .map(|(at, opened, _)| (*at..*at, opened.clone()))
        .collect();
    let (carets, opened, versions) = open_indented(model, &**host, (edits, origin.clone()), carets);
    // The row `o` or `O` opened, for `'.` if nothing's typed on it.
    if let Some((at, _, _)) = points.first().filter(|_| opens) {
        let row = row(&text, *at) + u32::from(args.at == InsertAt::LineBelow);
        let rows = (row, row);
        for version in [Some(versions.0), versions.1].into_iter().flatten() {
            state.marks.note(version, marks::Note::Put { rows });
        }
    }
    let opening = Opening {
        origin,
        count: args.count.unwrap_or(1).max(1),
        change: Change::Insert(args),
        opens,
        resumed: false,
        block: None,
    };
    enter(model, state, opening, carets, opened);
    Ok(Dispatched::new(Effect::Changed))
}

/// Opens a session over a block (`c`, `I` or `A`) for `change`, whose text
/// goes in `count` times: `edits` (sorted) open it, deleting the block or
/// making room on its first row, and typing starts at `at`, an offset in the
/// text after them, where undo puts the caret back. `rows` places the
/// block's other rows in that text, and the first row is followed from
/// there ([`FirstRow`]).
pub(crate) fn block(
    model: &mut EditorModel,
    state: &mut VimState,
    origin: Origin,
    (change, count): (Change, u32),
    (edits, at): (Vec<(Range<usize>, String)>, usize),
    rows: impl FnOnce(&BufferSnapshot) -> BlockRows,
) {
    let id = model.buffer_mut().start_transaction(origin.clone());
    let before = model.snapshot().clone();
    let (snapshot, events, followed, watched) = {
        let mut buffer = model.buffer_mut();
        buffer.edit(edits, origin.clone());
        let subscriptions = (buffer.subscribe(), buffer.subscribe(), buffer.subscribe());
        (
            buffer.snapshot(),
            subscriptions.0,
            subscriptions.1,
            subscriptions.2,
        )
    };
    let text = snapshot.text();
    let begin = history::pair(text, at);
    state.carets.record_pending(id, vec![begin]);
    state.marks.own(id);
    let point = text.offset_to_point(at);
    let start = text.row_start(point.row);
    // Typing at a row's start goes before its marks, as an ordinary
    // session's does: `edits` only change the row after that point.
    if at == start {
        let row_start = before.text().row_start(point.row);
        state.marks.typing(&before, [row_start]);
    }
    let first = FirstRow {
        events: followed,
        text: text.clone(),
        start,
        typed_ending: false,
        column: at - start,
        grown: 0,
        began: Some(at - start),
        leftmost: None,
        other: None,
        joined: false,
    };
    let opening = Opening {
        origin,
        count,
        change,
        opens: false,
        resumed: false,
        block: Some({
            let rows = rows(&snapshot);
            Block {
                watch: Arc::new(Mutex::new(Watch::new(watched, &rows))),
                rows,
                first: Arc::new(Mutex::new(first)),
            }
        }),
    };
    enter(model, state, opening, vec![(at, None)], (snapshot, events));
}

/// The text right after a session's opening edits, and a subscription
/// from there: what measuring text edits once the model catches up (see
/// [`apply`]) is the session's to follow.
type Opened = (BufferSnapshot, Subscription);

/// Each caret a session opens with, and where the indentation it made on
/// the caret's row starts, if it made any.
type Carets = Vec<(usize, Option<usize>)>;

/// Applies a session's opening `edits` (none for `i`, `a` and the like),
/// returning the snapshot right after them and a subscription from there
/// ([`Opened`]). The rows an opening makes or empties (`o`, `O`, a
/// linewise `c`) are then indented as `host` suggests for the buffer's
/// language, as nvim's are by a filetype's 'indentexpr'; where it
/// has no suggestion, a row keeps what it has (the row before's copied, or
/// its own). Each of `carets` (sorted, in the text right after `edits`)
/// with indentation, from its start to the caret, is on such a row.
///
/// The host answers from the text as `edits` left it, so the re-indenting
/// goes in after, as a batch of its own in the transaction, while the
/// buffer is still held, so nothing measuring text edits comes between.
/// The session follows the buffer from there. Returns the carets then, the
/// snapshot and subscription, and the versions `edits` and the
/// re-indenting (if any) left, for the marks.
fn open_indented(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    (edits, origin): (Vec<(Range<usize>, String)>, Origin),
    carets: Carets,
) -> (Carets, Opened, (Version, Option<Version>)) {
    let levels = Levels::of(model);
    let mut buffer = model.buffer_mut();
    buffer.edit(edits, origin.clone());
    let snapshot = buffer.snapshot();
    let opening = snapshot.version();
    let text = snapshot.text();
    let mut planned = HashMap::new();
    let mut edits = Vec::new();
    for &(head, indent) in &carets {
        let Some(start) = indent else {
            continue;
        };
        let row = row(text, start);
        if planned.contains_key(&row) {
            continue;
        }
        let Some(suggestion) = host.suggest_indent(&snapshot, row) else {
            continue;
        };
        let indent = suggested_indent(&levels, text, suggestion, &planned);
        if text.slice_to_string(start..head) != indent {
            edits.push((start..head, indent.clone()));
        }
        planned.insert(row, indent);
    }
    if edits.is_empty() {
        return (carets, (snapshot, buffer.subscribe()), (opening, None));
    }
    // Where each edit goes, and how long it is, once they're all in.
    let mut grown = 0isize;
    let moved: Vec<(Range<usize>, usize, usize)> = edits
        .iter()
        .map(|(range, indent): &(Range<usize>, String)| {
            let at = range.start.saturating_add_signed(grown);
            grown += indent.len() as isize - range.len() as isize;
            (range.clone(), at, indent.len())
        })
        .collect();
    // An offset the edits starting at it come after; inside one, its end.
    let after = |offset: usize| {
        let ix = moved.partition_point(|(range, _, _)| range.start < offset);
        match ix.checked_sub(1).map(|ix| &moved[ix]) {
            Some((range, at, len)) => at + len + offset.saturating_sub(range.end),
            None => offset,
        }
    };
    let carets = carets
        .iter()
        .map(|&(head, indent)| {
            let Some(start) = indent else {
                return (after(head), None);
            };
            let ix = moved.partition_point(|(range, _, _)| range.start < start);
            match moved.get(ix).filter(|(range, _, _)| range.start == start) {
                Some(&(_, at, len)) => (at + len, Some(at)),
                None => (after(head), Some(after(start))),
            }
        })
        .collect();
    buffer.edit(edits, origin);
    let snapshot = buffer.snapshot();
    let reindented = snapshot.version();
    (
        carets,
        (snapshot, buffer.subscribe()),
        (opening, Some(reindented)),
    )
}

/// How the buffer indents, for [`suggested_indent`]: a level's text
/// (`EditorModel::indent_unit`), the tab size and the indent style.
struct Levels {
    unit: String,
    tab: usize,
    style: Option<IndentStyle>,
}

impl Levels {
    fn of(model: &EditorModel) -> Self {
        Self {
            unit: model.indent_unit(),
            tab: model.tab_size().max(1),
            style: model.indent_style(),
        }
    }
}

/// The indentation `suggestion` gives, in `text`, as the editor's line
/// break makes it (`EditorModel::newline_in`): a level deeper is the basis
/// row's indentation and the buffer's indent unit; a level out is that
/// many fewer columns, in tabs as far as they go if the buffer indents
/// with tabs. The basis row's indentation is as `planned`, if there.
fn suggested_indent(
    levels: &Levels,
    text: &Text,
    suggestion: IndentSuggestion,
    planned: &HashMap<u32, String>,
) -> String {
    let IndentSuggestion::Relative { basis_row, delta } = suggestion else {
        return String::new();
    };
    let basis = indent_of(text, basis_row, planned);
    let count = delta.unsigned_abs() as usize;
    if delta >= 0 {
        return basis + &levels.unit.repeat(count);
    }
    let tab = levels.tab;
    let width = match levels.style {
        Some(IndentStyle::Spaces(n)) => usize::from(n),
        Some(IndentStyle::Tabs) | None => tab,
    };
    let columns = blank_width(&basis, 0, tab).saturating_sub(width * count);
    match levels.style {
        Some(IndentStyle::Tabs) => "\t".repeat(columns / tab) + &" ".repeat(columns % tab),
        _ => " ".repeat(columns),
    }
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
    if let Some(value) = write.filter(|_| !state.is_emptied(model)) {
        let small = !value.linewise && !value.text.contains('\n');
        let percent = args.target.writes_numbered();
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
    state.marks.own(id);
    let removed = merge(deletes);
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
    let edits = removed
        .iter()
        .map(|range| (range.clone(), String::new()))
        .collect();
    let (carets, opened, (_, reindented)) =
        open_indented(model, host, (edits, origin.clone()), carets);
    // Re-indenting sets no mark: the deletion's stand.
    if let Some(version) = reindented {
        state.marks.note(version, marks::Note::Unchanged);
    }
    let opening = Opening {
        origin,
        count: 1,
        change: Change::Operate(args),
        opens: false,
        resumed: false,
        block: None,
    };
    enter(model, state, opening, carets, opened);
    Ok(Dispatched::new(Effect::Changed))
}

/// Types `text` at every caret, as `.` replays a session, leaving each
/// caret after it. A row it starts with a closer then lines up with the
/// closer's opener, as `host` says, as typing the closer does
/// (`EditorModel::insert_typed_in`), in a batch of its own.
pub(crate) fn type_at_carets(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    text: &str,
    origin: Origin,
) {
    model.sync_display();
    let heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    let edits = heads.iter().map(|&head| (head..head, text));
    let (_, snapshot) = apply(model, edits, origin.clone());
    let starts: Vec<usize> = (heads.iter().enumerate())
        .map(|(ix, head)| head + ix * text.len())
        .collect();
    let after: Vec<usize> = starts.iter().map(|start| start + text.len()).collect();
    // Where the text went in, kept through what measuring text edits.
    let typed: Vec<(Anchor, Anchor)> = starts
        .iter()
        .map(|&start| {
            let end = start + text.len();
            (snapshot.anchor_after(start), snapshot.anchor_before(end))
        })
        .collect();
    place(model, &snapshot, &after);
    outdent_closers(model, host, &typed, origin);
}

/// Lines up each row whose first non-blank is in text `typed` went in
/// between, with the earlier row `host` says it does, as after a `}`
/// ([`EditorHost::closer_outdents`]): its indentation becomes that row's,
/// as this leaves it. It's planned from the text as it goes in
/// ([`apply_planned`]); the carets, anchored, move with it.
fn outdent_closers(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    typed: &[(Anchor, Anchor)],
    origin: Origin,
) {
    apply_planned(model, origin, |snapshot| {
        let text = snapshot.text();
        let mut planned: HashMap<u32, String> = HashMap::new();
        let mut edits = Vec::new();
        for (start, end) in typed {
            let typed = snapshot.resolve(start)..snapshot.resolve(end);
            for number in row(text, typed.start)..=row(text, typed.end) {
                let at = text.row_start(number);
                let first = at + indent_len(text, number);
                if !typed.contains(&first) || first == row_end(text, number) {
                    continue;
                }
                let Some(basis) = host
                    .closer_outdents(snapshot, number)
                    .filter(|&basis| basis < number)
                else {
                    continue;
                };
                let indent = indent_of(text, basis, &planned);
                if text.slice_to_string(at..first) != indent {
                    // With the closer, as the editor's is: the session
                    // takes the blanks for indentation ([`outdent`]).
                    let closer = text.slice_to_string(first..row_end(text, number));
                    let closer = closer.chars().next().expect("a non-blank");
                    let end = first + closer.len_utf8();
                    edits.push((at..end, format!("{indent}{closer}")));
                    planned.insert(number, indent);
                }
            }
        }
        edits
    });
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
    let VimTarget {
        model, state, host, ..
    } = target;
    let ended = end(model, state, Ending::Esc, Some(&**host));
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
/// the count's copies or the step back. `host` indents the rows `o` and
/// `O`'s copies open, at `Esc`. Returns the transaction it closed, if that
/// was kept.
fn end(
    model: &mut EditorModel,
    state: &mut VimState,
    how: Ending,
    host: Option<&dyn EditorHost>,
) -> Option<TransactionId> {
    let esc = matches!(how, Ending::Esc);
    state.mode = Mode::Normal;
    state.goals = None;
    // A replace session's copies and `.` type its keys again.
    let replace = state.replace.take();
    let mut session = state.session.take()?;
    let replaying = state.replaying;
    sync(model);
    let text = model.text().clone();
    let primary_start = session.regions.last().map(|region| region.start);
    // Whether the rows the session began on were typed on, and whether it
    // edited the text at all (not counting what `Esc` does).
    let typed = {
        let mut ledger = session.ledger();
        ledger.follow(&session.origin);
        match std::mem::replace(&mut ledger.end, End::Ending) {
            End::Open => Ok((
                primary_start.is_some_and(|start| {
                    let start = model.snapshot().resolve(&start);
                    ledger.indents.used_before(&text, start)
                }),
                ledger.edited,
            )),
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
    let (typed_first, edited) = match typed {
        Ok(typed) => typed,
        // A clone of this state ended it already: this state takes the
        // outcome. One ending it now (from inside measuring text) has none
        // to give yet. Either way, its model's carets are made normal mode's.
        Err(closed) => {
            match closed {
                Some(closed) => {
                    state.set_last_change(closed.last_change);
                    if !replaying {
                        state.carets.close(closed.transaction);
                    }
                    if let Some(left) = closed.left {
                        marks::adopt_insert(state, *left);
                    }
                }
                None => {
                    state.marks.closing();
                    state.closing = Some(Closing {
                        ledger: Arc::clone(&session.ledger),
                        replaying,
                        changes: state.changes,
                        carets: (!replaying).then(|| state.carets.take_pending()).flatten(),
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
    let keys = replace.map(|replace| replace.keys(model, primary));
    // Typing begins past the indentation the session made there, which a
    // closer's outdent may have rewritten from under the region's start.
    let begun = |start: &Anchor| session.ledger().indents.past(snapshot.resolve(start));
    let ended: Vec<Ended> = session
        .regions
        .iter()
        .map(|region| {
            let (start, end) = (begun(&region.start), snapshot.resolve(&region.end));
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
        .map(|start| begun(&start))
        .filter(|&start| primary >= start)
        .map(|start| {
            (
                snapshot.anchor_before(start),
                snapshot.anchor_before(primary),
            )
        });

    // Over a block, where this session typed on its first row, and at `Esc`
    // what goes on its other rows (see [`Block::typed`]).
    let block_typed = session.block.as_ref().and_then(|block| {
        let mut first = block.first();
        first.follow(&session.origin);
        first.end(edited);
        esc.then(|| block.typed(&first, model.primary_selection().head()))?
    });
    // Where `'.` goes in the count's copies at the primary caret.
    let mut copied = None;
    if esc {
        // Over a block, only as many copies as fit on all of its rows, with
        // what goes in on its other rows besides (see [`type_on_rows`]).
        if let (Some(block), Some(typed)) = (&session.block, &block_typed)
            && !block.rows.rows.is_empty()
        {
            let rows = block.rows.rows.len();
            let spaces = block.rows.rows.iter().map(|row| row.spaces.0);
            let room = MAX_PUT
                .saturating_sub(spaces.fold(0, usize::saturating_add))
                .saturating_sub(rows.saturating_mul(typed.prefix.len()));
            let each = typed.text.len().max(1).saturating_mul(rows + 1);
            let fit = u32::try_from((room / each).max(1)).unwrap_or(u32::MAX);
            session.count = session.count.min(fit);
        }
        copied = match &keys {
            Some((keys, _)) => {
                crate::replace::repeat_typed(model, &session, session.count, keys);
                None
            }
            None => repeat_typed(model, &session, &ended, host),
        };
    }
    let unindented = drop_unused_indent(model, &session, matches!(how, Ending::Moved(_)));
    // Where insert mode is left, for `'^`.
    let exit = esc.then(|| {
        model
            .snapshot()
            .anchor_before(model.primary_selection().head())
    });
    // Over a block, `Esc` types that on the others too, once the carets
    // stepped back, as nvim's does; `I` and `A` then leave the caret on the
    // block's corner.
    let corner = match (&session.block, &block_typed) {
        (Some(block), Some(typed)) => {
            step_back(model);
            let end = type_on_rows(model, &session, block, typed, session.count);
            Some((typed.corner, end))
        }
        _ => None,
    };
    // Leaving insert mode sets `'^` where it was left, and `'[` and `']`
    // from where typing began to that (over a block, but for `c`, to the end
    // of what went in on its last row), as nvim's does.
    // `'.` is where typing the last of the count's copies again left it
    // (see [`last_change`]), or where `Esc` took indentation out of the row
    // it's left on after them, or at the start of a block's first row typed
    // on, as nvim's are.
    let copied = copied.filter(|_| session.block.is_none());
    // A clone of this state that left the session to it reads these
    // batches as older than what it did since.
    for version in std::mem::take(&mut session.ledger().ending) {
        state.marks.note(version, marks::Note::Ending);
    }
    let left = exit.map(|exit| {
        let snapshot = model.snapshot();
        let exit = snapshot.resolve(&exit);
        let rows = corner.as_ref().and_then(|(_, rows)| rows.as_ref());
        let typed_end = match (&session.block, rows) {
            (Some(block), Some((end, _))) if !matches!(block.rows.edge, Edge::Fixed) => {
                Some(snapshot.resolve(end))
            }
            _ => None,
        };
        let change = match rows {
            Some((_, first)) => {
                let text = snapshot.text();
                Some(text.row_start(text.offset_to_point(snapshot.resolve(first)).row))
            }
            None => copied.map(|change| {
                let text = snapshot.text();
                let row = text.offset_to_point(exit).row;
                let on_row = |&at: &usize| text.offset_to_point(at).row == row;
                (unindented.iter().copied().find(on_row))
                    .unwrap_or_else(|| snapshot.resolve(&change))
            }),
        };
        let end = typed_end.unwrap_or(exit);
        Box::new(marks::left_insert(state, model, exit, end, change))
    });
    let corner = corner.map(|(corner, _)| corner);

    let snapshot = model.snapshot();
    let typed = match keys {
        Some((_, typed)) => typed,
        None => kept.map_or_else(String::new, |(start, end)| {
            let (start, end) = (snapshot.resolve(&start), snapshot.resolve(&end));
            model.text().slice_to_string(start..end.max(start))
        }),
    };
    let ended = model.buffer_mut().end_transaction();
    // An empty transaction isn't kept, so neither are its carets; inside
    // `.`, the transaction is `.`'s, which closes it.
    if !replaying {
        state.carets.close(ended);
    }

    if esc && corner.is_none() {
        step_back(model);
    }
    if let (Some(Some(column)), Some(block)) = (corner, &session.block) {
        let row = {
            let mut first = block.first();
            first.follow(&session.origin);
            row(&first.text, first.start)
        };
        let text = model.text().clone();
        let mut cursor = Cursor::at(&text, text.row_start(row));
        cursor.vertical(row, Some(column), model.tab_size().max(1));
        model.set_selections([caret(cursor.offset())]);
    }
    // A session a split resumed that edited nothing leaves `.` the change
    // before the split, as nvim does, whatever `Esc` typed on a block.
    let last_change = match &state.last_change {
        Some(last) if session.resumed && !edited => last.clone(),
        _ => LastChange {
            change: session.change.clone(),
            inserted: Some(typed),
            typed_first,
        },
    };
    session.ledger().end = End::Ended(Closed {
        last_change: last_change.clone(),
        transaction: ended,
        left,
    });
    state.set_last_change(last_change);
    ended
}

/// Steps each caret back one grapheme, as `Esc` does, unless it's at its
/// row's start.
fn step_back(model: &mut EditorModel) {
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

/// Types `typed`, `count` times as the first row took it, on `block`'s other
/// rows as `Esc` ends a session over it: on each that's as it was
/// ([`BlockRow`]), at its place or at `typed`'s column, once where rows
/// meet, and never on the first row or above it. At most [`MAX_PUT`] bytes
/// go in, spaces included; past that, none do. The carets keep their rows
/// and byte columns, as nvim's cursor does on a row typed on before it
/// (`c`), which then lands on what went in.
fn type_on_rows(
    model: &mut EditorModel,
    session: &Session,
    block: &Block,
    typed: &Typed,
    count: u32,
) -> Option<(Anchor, Anchor)> {
    let origin = &session.origin;
    sync(model);
    let rows = &block.rows;
    let snapshot = model.snapshot();
    let text = snapshot.text();
    let mut last = {
        let mut first = block.first();
        first.follow(origin);
        row_end(text, row(text, first.start))
    };
    // All at once: they share the version the session opened at.
    let anchors: Vec<Anchor> = rows.rows.iter().flat_map(BlockRow::anchors).collect();
    let mut anchors = snapshot.resolve_many(&anchors).into_iter();
    let mut places = Vec::new();
    {
        // Released before the model catches up below.
        let mut watch = block.watch();
        watch.follow(origin);
        for (ix, row) in rows.rows.iter().enumerate() {
            let found = row.found((text, &rows.text), &mut anchors);
            let (start, placed) = match (watch.fate(ix), found) {
                (Fate::Same | Fate::Lost, Some(start)) => (start, rows.place(row, typed.column)),
                (Fate::Typed, _) => {
                    let start = watch.start(ix);
                    let placed = block_rows::retyped(rows, row, (text, start), typed.column);
                    (start, placed)
                }
                _ => continue,
            };
            let Some((place, spaces)) = placed else {
                continue;
            };
            let range = start + place.start..start + place.end;
            if range.start <= last {
                continue;
            }
            last = range.end;
            places.push((range, spaces));
        }
    }
    let copies = count.max(1) as usize;
    let each = typed
        .text
        .len()
        .checked_mul(copies)
        .and_then(|each| each.checked_add(typed.prefix.len()));
    let size = places.iter().try_fold(0usize, |size, (_, spaces)| {
        size.checked_add(spaces.0)?.checked_add(each?)
    });
    if size.is_none_or(|size| size > MAX_PUT) {
        return None;
    }
    let typed = format!("{}{}", typed.prefix, typed.text.repeat(copies));
    let edits: Vec<(Range<usize>, String)> = places
        .into_iter()
        .map(|(range, spaces)| (range, visual::spaced(&typed, spaces)))
        .collect();
    // Where what goes in on the first row starts and on the last ends, once
    // it's in.
    let mut grown = 0isize;
    let mut ends = Vec::with_capacity(edits.len());
    for (range, text) in &edits {
        let start = range.start.saturating_add_signed(grown);
        ends.push((start, start + text.len()));
        grown += text.len() as isize - range.len() as isize;
    }
    let carets = history::pairs(model);
    let (_, snapshot) = apply(model, edits, origin.clone());
    session.ending(snapshot.version());
    let typed = ends
        .first()
        .zip(ends.last())
        .map(|(&(first, _), &(_, end))| {
            (snapshot.anchor_before(end), snapshot.anchor_after(first))
        });
    let text = snapshot.text();
    let heads: Vec<usize> = carets
        .into_iter()
        .map(|(row, column)| text.row_start(row) + column)
        .collect();
    place(model, &snapshot, &heads);
    typed
}

impl BlockRows {
    /// Where `Esc` types on `row`, from its start, and the spaces it takes
    /// (see [`BlockRow`]): at its place, or at display `column`, as `I`
    /// types there (`$A`: a row short of it at its end).
    fn place(
        &self,
        row: &BlockRow,
        column: Option<usize>,
    ) -> Option<(Range<usize>, (usize, usize))> {
        let Some(column) = column else {
            return Some((row.place.clone(), row.spaces));
        };
        let start = row.text.start;
        let ix = self.text.offset_to_point(start).row;
        match visual::insertion(&self.text, ix, (Some(column), false), self.tab, false) {
            Some((range, spaces, at)) => {
                Some((range.start - start..range.end - start, (spaces, at)))
            }
            None => matches!(self.edge, Edge::End(_)).then(|| {
                let end = row.text.len();
                (end..end, (0, 0))
            }),
        }
    }
}

/// A count's copies at one caret, for [`repeat_typed`].
struct Put {
    /// The caret whose typed text they copy.
    head: usize,
    /// Where they go.
    at: usize,
    text: String,
    /// Where `'.` goes in `text` ([`last_change`]).
    change: usize,
    /// For `o` and `O`, the rows the copies open, if they're indented, for
    /// the host to indent ([`indent_copies`]).
    opened: Option<Reopened>,
    /// What they are, for the session: their start is set once they're in.
    copies: Copies,
}

/// The rows a caret's copies of `o` or `O` open, for [`indent_copies`].
#[derive(Copy, Clone, Debug)]
struct Reopened {
    /// How many rows each copy has.
    rows: u32,
    /// How many copies there are.
    count: usize,
    /// The blanks each copy's first row starts with, typed after the
    /// indentation the row was opened with.
    lead: usize,
    /// How many more bytes indenting them may put in, within [`MAX_PUT`]
    /// over all the carets, as the copies are counted.
    room: usize,
}

/// The last row of a copy, after its last line break, for `'.`: how much
/// of the indentation the session made starts it, and whether nvim, typing
/// the copy again, would have indented it only for `Esc` to take that out.
#[derive(Copy, Clone, Debug, Default)]
struct Tail {
    lead: usize,
    stripped: bool,
}

impl Tail {
    /// With `lead` the indentation the session made at the row's start,
    /// unless that's taken out of the copy.
    fn lead(self, lead: usize) -> Self {
        Self {
            lead: if self.stripped { 0 } else { lead },
            ..self
        }
    }
}

/// Where `'.` goes in a caret's copies, `text`, the last of which starts at
/// `last`, as nvim's typing them again leaves it: at the text typed after
/// the last copy's last line break (past the indentation it got), else at
/// that row's start if `Esc` took indentation out there, else at the line
/// break; at the last copy's start if it has none.
fn last_change(text: &str, last: usize, tail: Tail) -> usize {
    let copy = &text[last..];
    match copy.rfind('\n') {
        None => last,
        Some(at) => {
            let row = last + at + 1;
            match () {
                _ if text.len() - row > tail.lead => row + tail.lead,
                _ if tail.stripped => row,
                _ => row - 1,
            }
        }
    }
}

/// Puts each region's typed text in `count - 1` more times at its caret,
/// leaving the caret after the copies: at the caret, or for `o` and `O` in
/// new rows below its row. The copies hold the session's indentation as the
/// typed text does, but for what `Esc` would remove from them, and type on
/// the row each one starts on as typing them would; the rows `o` and `O`
/// open get indentation unless it would go unused. The session is told
/// what they are before they go in, as measuring text while the model
/// catches up may follow their batch (see [`Ledger`]). `host` then indents
/// the rows `o` and `O` open, as it suggests ([`indent_copies`]). Returns
/// where `'.` goes in the last region's copies, if it has any
/// ([`last_change`]).
fn repeat_typed(
    model: &mut EditorModel,
    session: &Session,
    ended: &[Ended],
    host: Option<&dyn EditorHost>,
) -> Option<Anchor> {
    let more = session.count as usize - 1;
    if more == 0 {
        return None;
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
                let lead = ledger.indents.lead_in(region.start..head, typed);
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
                        let (copies, last, count) = reopened(typed, &copy.each, rows, more, carets);
                        // One row typed is the whole copy's last: nvim
                        // indents it as the row before (that indentation
                        // going at `Esc` if nothing's typed past it).
                        let tail = copy.each_tail.lead(lead);
                        let tail = match typed.contains('\n') {
                            true => tail,
                            false => {
                                let indent = copies[last..].len() - copy.each.len() - 1;
                                let blank = copy.each.len() <= tail.lead;
                                Tail {
                                    lead: indent + tail.lead,
                                    stripped: blank && (rows.width > 0 || tail.stripped),
                                }
                            }
                        };
                        let opened = rows.indented.then(|| Reopened {
                            rows: typed.split('\n').count() as u32,
                            count,
                            lead: blank_len(&copy.each),
                            room: (MAX_PUT / carets).saturating_sub(copies.len()),
                        });
                        Some(Put {
                            head,
                            at: row_end(&text, row),
                            change: last_change(&copies, last, tail),
                            opened,
                            text: copies,
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
                            change: last_change(&copies, before, copy.last_tail.lead(lead)),
                            opened: None,
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
        return None;
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
    let primary = ended.last().and_then(|region| region.head);
    let change = puts
        .iter()
        .find(|put| Some(put.head) == primary)
        .map(|put| put.copies.start + put.change);
    let opened: Vec<(Range<usize>, Reopened)> = puts
        .iter()
        .filter_map(|put| {
            let start = put.copies.start;
            Some((start..start + put.text.len(), put.opened?))
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
    session.ending(snapshot.version());
    place(model, &snapshot, &carets);
    let change = change.map(|offset| snapshot.anchor_before(offset));
    if let Some(host) = host {
        indent_copies(model, session, host, &snapshot, &opened);
    }
    change
}

/// Indents the rows `o` and `O`'s copies open as `host` suggests for the
/// buffer's language, as nvim's do, opening each with its 'indentexpr',
/// top to bottom, each from the rows above as this leaves them. A row's
/// indentation is what it was opened with, before the blanks typed after
/// it, which stay. Where the host has no suggestion, a row keeps the width
/// of the row before, as the copies have it, or as this re-indents it.
///
/// Each of `opened` (in order) is where a caret's copies lie in
/// `snapshot`, the text they left, from the line break before the first.
/// What indenting them puts in counts towards [`MAX_PUT`] as the copies
/// do: the copies it has no room for go, as those `reopened` has none for
/// never go in. The host answers from the text as the copies left it, so
/// this goes in after them, as a batch of its own, planned from the text
/// as it goes in ([`apply_planned`]): copies another origin wrote inside
/// meanwhile, as measuring text may, are left as they are. The carets,
/// anchored, move with it.
fn indent_copies(
    model: &mut EditorModel,
    session: &Session,
    host: &dyn EditorHost,
    snapshot: &BufferSnapshot,
    opened: &[(Range<usize>, Reopened)],
) {
    let levels = Levels::of(model);
    let since = snapshot.version();
    let indented = apply_planned(model, session.origin.clone(), |snapshot| {
        let text = snapshot.text();
        // The rows re-indented, with all their blanks: the rest are as the
        // text has them, which keeps this small for a large count.
        let mut planned: HashMap<u32, String> = HashMap::new();
        let mut edits = Vec::new();
        for (copies, reopened) in opened {
            let Some(copies) = untouched(snapshot, since, copies.clone()) else {
                continue;
            };
            let first = row(text, copies.start) + 1;
            let mut room = reopened.room;
            for copy in 0..reopened.count as u32 {
                let row = first + copy * reopened.rows;
                let start = text.row_start(row);
                let blanks = start..start + indent_len(text, row);
                let opening = blanks.start..blanks.end.saturating_sub(reopened.lead);
                let indent = match host.suggest_indent(snapshot, row) {
                    Some(suggestion) => suggested_indent(&levels, text, suggestion, &planned),
                    None => match planned.get(&(row - 1)) {
                        Some(before) => " ".repeat(blank_width(before, 0, levels.tab)),
                        None => continue,
                    },
                };
                if text.slice_to_string(opening.clone()) == indent {
                    continue;
                }
                let grows = indent.len().saturating_sub(opening.len());
                if grows > room {
                    edits.push((start - 1..copies.end, String::new()));
                    break;
                }
                room = room - grows + opening.len().saturating_sub(indent.len());
                let typed = text.slice_to_string(opening.end..blanks.end);
                planned.insert(row, format!("{indent}{typed}"));
                edits.push((opening, indent));
            }
        }
        edits
    });
    if let Some(snapshot) = indented {
        session.ending(snapshot.version());
    }
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
        each_tail: Tail::default(),
        last_tail: Tail::default(),
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
        if row + 1 == rows {
            // The indentation the row starts with, kept or taken out.
            let mut lead = at;
            for piece in pieces {
                if piece.range.start != lead {
                    break;
                }
                lead = piece.range.end;
            }
            let made = lead > at;
            // The lead is set from all the indentation made ([`Tail::lead`]).
            let tail = |strip: bool| Tail {
                lead: 0,
                stripped: strip && made,
            };
            copy.each_tail = tail(removed(false) == Some(true));
            copy.last_tail = tail(removed(true) == Some(true));
        }
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
/// `carets` carets, counted as typing `typed` again would put them in;
/// with where the last of them starts and how many there are.
fn reopened(
    typed: &str,
    each: &str,
    mut rows: Rows,
    more: usize,
    carets: usize,
) -> (String, usize, usize) {
    let last = typed
        .rsplit_once('\n')
        .map(|(_, last)| blank_width(last, 0, rows.tab));
    let (mut copies, mut size, mut start) = (String::new(), 0usize, 0);
    let mut count = 0;
    for _ in 0..more {
        let indent = rows.next(typed, last);
        size += 1 + indent + typed.len();
        if size.saturating_mul(carets) > MAX_PUT {
            break;
        }
        start = copies.len();
        count += 1;
        copies.push('\n');
        copies.extend(std::iter::repeat_n(' ', indent));
        copies.push_str(each);
    }
    (copies, start, count)
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

/// The byte length of the blanks `line` starts with.
fn blank_len(line: &str) -> usize {
    line.bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .count()
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
/// (their goal columns too) if nothing before them went. Returns where
/// each piece taken out was, in the text after.
fn drop_unused_indent(
    model: &mut EditorModel,
    session: &Session,
    off_caret_rows: bool,
) -> Vec<usize> {
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
        return Vec::new();
    }
    let mut gone = 0;
    let unindented = removed
        .iter()
        .map(|range| {
            gone += range.len();
            range.end - gone
        })
        .collect();
    let ends: Vec<usize> = selections
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .collect();
    let mapped = through_deletes(&ends, &removed);
    let edits = removed.iter().map(|range| (range.clone(), ""));
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    session.ending(snapshot.version());
    if mapped == ends {
        return unindented;
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
    unindented
}
