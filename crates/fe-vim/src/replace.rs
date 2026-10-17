//! Replace mode (`R`, `:help Replace-mode`): `vim.replace_mode` opens an
//! insert session as `i` does (one transaction, `.` repeating it), in which
//! typed text overwrites the row instead of going in before it.
//!
//! The host types through [`VimState::replace_text`], `Enter` through
//! [`VimState::line_break`] and `<BS>` through
//! [`VimState::replace_backspace`]. Each character typed overwrites the
//! grapheme under the caret, or goes in past the row's end; a line break
//! overwrites nothing, copies its row's indentation and drops the blanks it
//! moves to the new row, as nvim's 'autoindent' does. Each key is kept as an
//! [`Entry`]: what it put in, followed through every edit since, and what
//! it took out, so `<BS>` puts that back, and only while what it put in is
//! still there, untouched by another origin, just before the caret; and the
//! keys `<BS>` took back where typing them again elsewhere may leave
//! something (a combining mark joins whatever blank it follows). `Esc`
//! (`vim.replace_normal_mode`) ends the session as `vim.normal_mode` does,
//! but for the count's copies, which type the session's keys again
//! ([`repeat_typed`]), as `.` does ([`retype`]).

use std::{
    collections::VecDeque,
    fmt,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use fe_buffer::{BufferEvent, Edit, EditKind, Origin, Subscription, TransactionId, Version};
use fe_command::{Actor, CommandError, Dispatched};
use fe_editor::{EditorModel, commands::origin};
use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Change, CountArgs, InsertArgs, InsertAt, Mode, VimState, VimTarget,
    insert::{self, Session},
    motion::Line,
    operator::{MAX_PUT, apply, caret, row_end, sync},
    visual::width_from,
};

#[cfg(test)]
thread_local! {
    /// How many edits the last count's copies made on this thread: one a
    /// caret, whatever the count.
    pub(crate) static COPIES_EDITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `<BS>` in the keys a session typed: after a line break, the indentation
/// it copied taken out again.
const BACKSPACE: char = '\u{8}';

/// How many characters of the grapheme a copy is typed after
/// ([`Typing::context`]) it keeps: enough to pair a regional indicator or
/// join an emoji sequence, few enough that a grapheme the copies keep
/// growing ends the same after a few of them.
const CONTEXT: usize = 16;

/// What a replace session's typing did, for `<BS>`, the count's copies and
/// `.`. Shared by clones of the state, as [`VimState::line_break`] takes it
/// by reference.
#[derive(Clone, Debug)]
pub(crate) struct Replace {
    record: Arc<Mutex<Record>>,
    /// A virtual session (`gR`): typed characters replace screen columns.
    virtual_: bool,
}

struct Record {
    /// The buffer's edits, followed into each entry.
    events: Subscription,
    /// The session's origin: anyone else's edit is another's.
    origin: Origin,
    /// What was typed at one caret, oldest first: a caret's is the one
    /// whose last entry it's just after.
    typed: Vec<Vec<Entry>>,
}

impl fmt::Debug for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Record")
            .field("typed", &self.typed)
            .finish_non_exhaustive()
    }
}

/// One key's worth of typing in replace mode.
#[derive(Clone, Debug)]
struct Entry {
    /// Where what it put in is, in the text as of the last edit followed.
    made: Range<usize>,
    /// A character, or a line break and the indentation it copied (what
    /// `<BS>` left of it); nothing for keys alone, which `<BS>` took back
    /// at the record's start.
    put: String,
    /// What it took out: the grapheme a character overwrote (nothing past
    /// the row's end), or the blanks a line break dropped.
    was: String,
    /// The keys typed after it that `<BS>` took back again, [`BACKSPACE`]s
    /// included, where typing them again elsewhere may leave something
    /// ([`Undo::apply`]): after a line break, characters (with the last
    /// blank of its indentation, if they joined it: `<BS>` takes a
    /// grapheme) and that indentation; after any key, a line break.
    then: String,
    /// Another origin's edit, an undo or a redo touched what it put in.
    touched: bool,
    /// Spaces right after `made` that a virtual session's character left
    /// of a wide one it only partly covered, which `<BS>` takes out too.
    trail: usize,
    /// What the indentation of a virtual session's line break replaced on
    /// the next row, which `<BS>` puts back as it takes the indentation out.
    indent_was: String,
    /// The first of this many entries a typed tab became in a virtual
    /// session (spaces), or one of the rest of them.
    tab: usize,
    tab_rest: bool,
}

impl Entry {
    fn new(made: Range<usize>, put: String, was: String) -> Self {
        Self {
            made,
            put,
            was,
            then: String::new(),
            touched: false,
            trail: 0,
            indent_was: String::new(),
            tab: 0,
            tab_rest: false,
        }
    }

    fn is_break(&self) -> bool {
        self.put.starts_with('\n')
    }

    /// Whether what it put in is still there, as it put it.
    fn live(&self, text: &Text) -> bool {
        !self.touched
            && self.made.end <= text.len()
            && text.slice_to_string(self.made.clone()) == self.put
    }

    /// The keys that typed it.
    fn keys(&self, out: &mut String) {
        match self.is_break() {
            true => out.push('\n'),
            false => out.push_str(&self.put),
        }
        out.push_str(&self.then);
    }

    /// Whether replaying its keys and a `<BS>` taking it out again may
    /// leave something: not for a key alone, nor a line break whose
    /// indentation `<BS>` took out whole.
    fn leaves_keys(&self) -> bool {
        self.then.chars().any(|key| key != BACKSPACE)
    }
}

/// Typing going in as the batch from `from`: each caret's record, if it had
/// one, and its entries, placed in the text right after that batch.
struct Pending {
    from: Version,
    typed: Vec<(Option<usize>, Vec<Entry>)>,
}

impl Record {
    /// Follows the buffer's edits since the last call into every entry,
    /// adding `pending`'s when its batch comes.
    fn follow(&mut self, mut pending: Option<Pending>) {
        for event in self.events.drain() {
            let BufferEvent::Edited {
                origin,
                kind,
                from,
                edits,
                ..
            } = event
            else {
                continue;
            };
            let other = origin != self.origin || kind != EditKind::Edit;
            for entry in self.typed.iter_mut().flatten() {
                entry.touched |= other && touches(&edits, &entry.made);
                // Keys whose text is all gone stay before text put in there.
                let start = map(&edits, entry.made.start, !entry.made.is_empty());
                entry.made = start..map(&edits, entry.made.end, false).max(start);
            }
            if let Some(pending) = pending.take_if(|pending| pending.from == from) {
                for (record, entries) in pending.typed {
                    match record.and_then(|ix| self.typed.get_mut(ix)) {
                        Some(record) => record.extend(entries),
                        None => self.typed.push(entries),
                    }
                }
            }
        }
    }

    /// The record whose last entry is live and ends at `head`, after
    /// dropping the entries at each record's end whose text another's
    /// delete took out.
    fn find(&mut self, text: &Text, head: usize) -> Option<usize> {
        for record in &mut self.typed {
            while record
                .last()
                .is_some_and(|entry| entry.made.is_empty() && !entry.put.is_empty())
            {
                record.pop();
            }
        }
        self.typed.retain(|record| !record.is_empty());
        self.typed.iter().position(|record| {
            record
                .last()
                .is_some_and(|entry| entry.live(text) && entry.made.end == head)
        })
    }
}

/// Whether `edits` change any of `range`'s bytes or put text inside it.
fn touches(edits: &[Edit], range: &Range<usize>) -> bool {
    !range.is_empty()
        && edits.iter().any(|edit| {
            (edit.old.start < range.end && range.start < edit.old.end)
                || (edit.old.is_empty()
                    && range.start < edit.old.start
                    && edit.old.start < range.end)
        })
}

/// Where `at` goes through a batch of `edits`: past text put in at it if
/// `after`, else before it; past a replacement ending at it, to the start
/// of one it starts or is inside.
fn map(edits: &[Edit], at: usize, after: bool) -> usize {
    let mut shift = 0isize;
    for edit in edits {
        if edit.old.end < at || (edit.old.end == at && !edit.old.is_empty()) {
            shift = edit.new.end as isize - edit.old.end as isize;
            continue;
        }
        if edit.old.start > at {
            break;
        }
        match edit.old.is_empty() && after {
            true => shift = edit.new.end as isize - edit.old.end as isize,
            false => return edit.new.start,
        }
    }
    (at as isize + shift) as usize
}

impl Replace {
    fn new(model: &mut EditorModel, origin: Origin, virtual_: bool) -> Self {
        let events = model.buffer_mut().subscribe();
        Self {
            record: Arc::new(Mutex::new(Record {
                events,
                origin,
                typed: Vec::new(),
            })),
            virtual_,
        }
    }

    /// Whether it's a virtual session (`gR`).
    pub(crate) fn is_virtual(&self) -> bool {
        self.virtual_
    }

    fn record(&self) -> MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// For each of `heads`, the record typed just before it, if any, once
    /// the edits made since the last call are followed.
    fn at_carets(&self, text: &Text, heads: &[usize]) -> Vec<Option<usize>> {
        let mut record = self.record();
        record.follow(None);
        heads.iter().map(|&head| record.find(text, head)).collect()
    }

    /// The keys each caret typed (`\n` for `Enter`, then [`BACKSPACE`] if
    /// its indentation went), by its head, and those of the caret at
    /// `primary`, for `.`.
    pub(crate) fn keys(
        &self,
        model: &EditorModel,
        primary: usize,
    ) -> (Vec<(usize, String)>, String) {
        let text = model.text().clone();
        let mut record = self.record();
        record.follow(None);
        let mut keys_at = |head: usize| {
            let mut keys = String::new();
            if let Some(ix) = record.find(&text, head) {
                keys_of(&record.typed[ix], &mut keys);
            }
            keys
        };
        let heads = sorted_heads(model)
            .into_iter()
            .map(|head| (head, keys_at(head)))
            .collect();
        (heads, keys_at(primary))
    }
}

/// The keys that typed `entries`, a typed tab whole as the one key.
fn keys_of(entries: &[Entry], out: &mut String) {
    let mut at = 0;
    while let Some(entry) = entries.get(at) {
        let whole = entry.tab > 0
            && entries
                .get(at + 1..at + entry.tab)
                .is_some_and(|rest| rest.iter().all(|entry| entry.tab_rest));
        match whole {
            true => {
                out.push('\t');
                at += entry.tab;
            }
            false => {
                entry.keys(out);
                at += 1;
            }
        }
    }
}

/// `vim.replace_mode`: an insert session at each caret, as `i` opens one,
/// whose typing overwrites; `count` is how many times the text goes in.
pub(crate) fn replace(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    open(target, actor, args, false)
}

/// `vim.virtual_replace_mode`: [`replace`], whose typed characters replace
/// the screen columns they take.
pub(crate) fn virtual_replace(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    open(target, actor, args, true)
}

fn open(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
    virtual_: bool,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let insert = InsertArgs {
        at: InsertAt::Before,
        count: args.count,
    };
    let dispatched = insert::insert(target, actor, insert)?;
    if let Some(session) = &mut target.state.session {
        session.change = match virtual_ {
            true => Change::VirtualReplace(args),
            false => Change::Replace(args),
        };
    }
    enter(target.state, target.model, origin(actor), virtual_);
    Ok(dispatched)
}

/// Turns the open insert session into a replace session with nothing
/// typed yet, after a split resumed one ([`VimState::split_session`]),
/// which `.` repeats as `R`, or `gR` if it was one.
pub(crate) fn resumed(state: &mut VimState, model: &mut EditorModel, virtual_: bool) {
    let Some(session) = &mut state.session else {
        return;
    };
    let args = CountArgs { count: None };
    session.change = match virtual_ {
        true => Change::VirtualReplace(args),
        false => Change::Replace(args),
    };
    let origin = session.origin.clone();
    enter(state, model, origin, virtual_);
}

fn enter(state: &mut VimState, model: &mut EditorModel, origin: Origin, virtual_: bool) {
    state.mode = Mode::Replace;
    state.replace = Some(Replace::new(model, origin, virtual_));
}

/// The carets' heads, in order and once each.
fn sorted_heads(model: &EditorModel) -> Vec<usize> {
    let mut heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    heads.sort_unstable();
    heads.dedup();
    heads
}

/// The graphemes from `head` (a grapheme start or its row's end) to the
/// row's end: the row, and the index of the first.
fn rest_of_row(text: &Text, head: usize) -> (Line, usize) {
    let line = Line::new(text, text.offset_to_point(head).row);
    let column = head - line.start;
    let ix = line.starts.partition_point(|&start| start < column);
    (line, ix)
}

/// The offset `count` graphemes on from grapheme `ix` of `line`, or its
/// end.
fn past(line: &Line, ix: usize, count: usize) -> usize {
    let column = ix
        .checked_add(count)
        .and_then(|ix| line.starts.get(ix).copied())
        .unwrap_or(line.text.len());
    line.start + column
}

/// One caret's edit, and where it lands: this far into the edit's new text.
struct Act {
    range: Range<usize>,
    text: String,
    land: usize,
}

impl Act {
    fn stay(at: usize) -> Self {
        Self {
            range: at..at,
            text: String::new(),
            land: 0,
        }
    }
}

/// Applies each caret's [`Act`] (in order) with `origin` and places the
/// carets, returning where each act's text starts in the text right after
/// them, or `None` for one left out: an edit overlapping one before it,
/// whose caret lands where that one's does. Each goes in as its text put in
/// at its start and what it replaces taken out after that.
fn act(model: &mut EditorModel, origin: Origin, acts: Vec<Act>) -> Vec<Option<usize>> {
    act_on(model, origin, acts).0
}

/// [`act`], with the version its batch made, if it made one.
fn act_on(
    model: &mut EditorModel,
    origin: Origin,
    acts: Vec<Act>,
) -> (Vec<Option<usize>>, Option<Version>) {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut heads: Vec<usize> = Vec::with_capacity(acts.len());
    let mut starts = Vec::with_capacity(acts.len());
    let mut end = 0;
    let mut shift = 0isize;
    for act in acts {
        if act.range.start < end && !edits.is_empty() {
            heads.push(heads.last().copied().unwrap_or(0));
            starts.push(None);
            continue;
        }
        let start = (act.range.start as isize + shift) as usize;
        heads.push(start + act.land);
        starts.push(Some(start));
        shift += act.text.len() as isize - act.range.len() as isize;
        end = act.range.end.max(end);
        let at = act.range.start;
        if !act.text.is_empty() {
            edits.push((at..at, act.text));
        }
        if !act.range.is_empty() {
            edits.push((act.range, String::new()));
        }
    }
    if edits.is_empty() {
        model.set_selections(heads.into_iter().map(caret));
        return (starts, None);
    }
    let (_, snapshot) = apply(model, edits, origin);
    insert::place(model, &snapshot, &heads);
    (starts, Some(snapshot.version()))
}

/// Each caret's planned entries (offsets from its act's start), placed
/// where its act went in, for those that did.
fn placed(
    starts: Vec<Option<usize>>,
    planned: Vec<Vec<Entry>>,
    records: Vec<Option<usize>>,
) -> Vec<(Option<usize>, Vec<Entry>)> {
    starts
        .into_iter()
        .zip(planned)
        .zip(records)
        .filter_map(|((start, entries), record)| {
            let start = start?;
            let entries = entries
                .into_iter()
                .map(|mut entry| {
                    entry.made = start + entry.made.start..start + entry.made.end;
                    entry
                })
                .collect();
            Some((record, entries))
        })
        .collect()
}

/// `text` with every line ending a `\n`, as the buffer keeps them.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Types `keys` (a session's, see [`Replace::keys`]) at every caret, as `.`
/// replays a replace session: characters overwriting, `\n` as `Enter`,
/// [`BACKSPACE`] as `<BS>`.
pub(crate) fn retype(state: &VimState, model: &mut EditorModel, keys: &str) {
    let mut run = String::new();
    for key in keys.chars() {
        if key != '\n' && key != BACKSPACE {
            run.push(key);
            continue;
        }
        if !run.is_empty() {
            state.overwrite(model, &std::mem::take(&mut run));
        }
        match key {
            '\n' => state.line_break(model),
            _ => state.replace_backspace(model),
        }
    }
    if !run.is_empty() {
        state.overwrite(model, &run);
    }
}

/// [`VimState::line_break`] in replace mode: at every caret, a line break
/// and the indentation of the caret's row up to it, dropping the blanks
/// after the caret, as nvim's 'autoindent' does.
pub(crate) fn line_break(state: &VimState, model: &mut EditorModel) {
    let (Some(replace), Some(session)) = (&state.replace, &state.session) else {
        return;
    };
    sync(model);
    let text = model.text().clone();
    let heads = sorted_heads(model);
    let records = replace.at_carets(&text, &heads);
    let tab = model.tab_size().max(1);
    // In a virtual session, the indentation the caret's last key (a line
    // break) typed with nothing after it, which the next line break takes
    // out again.
    let unused: Vec<usize> = match replace.virtual_ {
        true => {
            let record = replace.record();
            records
                .iter()
                .map(|&ix| {
                    ix.and_then(|ix| record.typed[ix].last())
                        .filter(|entry| entry.is_break())
                        .map_or(0, |entry| entry.put.len() - 1)
                })
                .collect()
        }
        false => vec![0; heads.len()],
    };
    let mut planned = Vec::with_capacity(heads.len());
    let mut added = false;
    let acts = heads
        .iter()
        .zip(&unused)
        .map(|(&head, &unused)| {
            if replace.virtual_ {
                let (act, entry, adds) = virtual_break(&text, head, unused, tab);
                added |= adds;
                planned.push(vec![entry]);
                return act;
            }
            let row = text.offset_to_point(head).row;
            let line = text.row_to_string(row);
            let column = head - text.row_start(row);
            let indent = (line.len() - line.trim_start().len()).min(column);
            let after = &line[column..];
            let dropped = &after[..after.len() - after.trim_start().len()];
            let put = format!("\n{}", &line[..indent]);
            planned.push(vec![Entry::new(
                0..put.len(),
                put.clone(),
                dropped.to_string(),
            )]);
            Act {
                range: head..head + dropped.len(),
                land: put.len(),
                text: put,
            }
        })
        .collect();
    // The session takes the batch for a typed line break, whose white space
    // after the break is the new row's indentation (see `insert::Ledger`).
    // A virtual one that goes on to the next row types no new indentation:
    // it replaces the row's own, which `Esc` leaves.
    let from = model.buffer().version();
    let adds = !replace.virtual_ || added;
    if adds {
        session.typed_line_break(from);
    }
    let starts = act(model, session.origin.clone(), acts);
    if adds && model.buffer().version() == from {
        session.forget_line_break(from);
    }
    let typed = placed(starts, planned, records);
    replace.record().follow(Some(Pending { from, typed }));
}

/// A line break typed in a virtual session at `head`: the rest of its row
/// is deleted (with the `unused` bytes of indentation just before the
/// caret) and typing goes on at the start of the next row, where the
/// indentation of the caret's row replaces as many screen columns as typed
/// spaces would; past the last row a row is added instead, as `R`'s break
/// does. Whether it adds a row is the third.
fn virtual_break(text: &Text, head: usize, unused: usize, tab: usize) -> (Act, Entry, bool) {
    let row = text.offset_to_point(head).row;
    let line = Line::new(text, row);
    let before = &line.text[..head - line.start];
    let indent = &before[..before.len() - before.trim_start().len()];
    let start = head - unused.min(before.len());
    let end = line.start + line.text.len();
    let rest = &text.slice_to_string(start..end);
    let last = row + 1 >= text.row_count();
    let next = match last {
        true => Line::of(end, String::new()),
        false => Line::new(text, row + 1),
    };
    let typed = virtual_over(&next, 0, 0, indent, 1, tab, true);
    let put = format!("\n{}", &typed.text[..typed.land]);
    let mut entry = Entry::new(0..put.len(), put, rest.clone());
    let range = match last {
        true => start..end,
        false => {
            entry.was.push('\n');
            entry.indent_was = next.text[..past(&next, 0, typed.over) - next.start].to_string();
            entry.trail = typed.text.len() - typed.land;
            start..past(&next, 0, typed.over)
        }
    };
    let text = match last {
        true => entry.put.clone(),
        false => format!("\n{}", typed.text),
    };
    let act = Act {
        range,
        land: entry.put.len(),
        text,
    };
    (act, entry, last)
}

impl VimState {
    /// Types `text` at every caret in replace mode, as the host does for
    /// typed text there: each character overwrites the grapheme under the
    /// caret, or goes in past the row's end, and a line break (any line
    /// ending) is typed as `Enter` is ([`VimState::line_break`]),
    /// overwriting nothing. It joins the session's transaction, and
    /// [`VimState::replace_backspace`] can take it back. Outside replace
    /// mode it types `text` as [`EditorModel::insert`] does.
    pub fn replace_text(&self, model: &mut EditorModel, text: &str) {
        if self.mode != Mode::Replace || self.replace.is_none() {
            model.insert(text);
            return;
        }
        for (ix, row) in normalized(text).split('\n').enumerate() {
            if ix > 0 {
                self.line_break(model);
            }
            if !row.is_empty() {
                self.overwrite(model, row);
            }
        }
    }

    /// Whether the open session is a virtual replace one (`gR`), whose
    /// `Tab` the host types through [`VimState::replace_text`].
    pub fn is_virtual_replace(&self) -> bool {
        self.mode == Mode::Replace && self.replace.as_ref().is_some_and(Replace::is_virtual)
    }

    /// Types `row`, holding no line break, over every caret's row.
    fn overwrite(&self, model: &mut EditorModel, row: &str) {
        let (Some(replace), Some(session)) = (&self.replace, &self.session) else {
            return;
        };
        sync(model);
        let text = model.text().clone();
        let heads = sorted_heads(model);
        let records = replace.at_carets(&text, &heads);
        let mut planned: Vec<Vec<Entry>> = Vec::with_capacity(heads.len());
        let tab = model.tab_size().max(1);
        let acts = heads
            .iter()
            .map(|&head| {
                let (line, mut ix) = rest_of_row(&text, head);
                if replace.virtual_ {
                    let column = width_from(&line.text[..head - line.start], 0, tab);
                    let typed = virtual_over(&line, ix, column, row, 1, tab, true);
                    planned.push(typed.entries);
                    return Act {
                        range: head..past(&line, ix, typed.over),
                        text: typed.text,
                        land: typed.land,
                    };
                }
                let mut entries = Vec::new();
                for (at, ch) in row.char_indices() {
                    let was = match ix < line.len() {
                        true => {
                            ix += 1;
                            line.grapheme(ix - 1).to_string()
                        }
                        false => String::new(),
                    };
                    let made = at..at + ch.len_utf8();
                    entries.push(Entry::new(made, ch.to_string(), was));
                }
                planned.push(entries);
                Act {
                    range: head..past(&line, ix, 0),
                    text: row.to_string(),
                    land: row.len(),
                }
            })
            .collect();
        let from = model.buffer().version();
        let starts = act(model, session.origin.clone(), acts);
        let typed = placed(starts, planned, records);
        replace.record().follow(Some(Pending { from, typed }));
    }

    /// `<BS>` in replace mode, at every caret where the last key typed
    /// there put in what's still just before it, untouched by another
    /// origin. A character goes, and what it overwrote, if anything, goes
    /// back in its place, the caret before it; with it goes the rest of the
    /// grapheme before the caret, if the session typed all of that (as
    /// nvim's backspace takes a whole grapheme), the last blank of a line
    /// break's indentation among it. A line break takes out the indentation
    /// it copied, then itself, putting back the blanks it dropped. Anywhere
    /// else (where the session began, or text another origin changed) it
    /// does nothing. Outside replace mode it's [`EditorModel::backspace`].
    pub fn replace_backspace(&self, model: &mut EditorModel) {
        let (Some(replace), Some(session), Mode::Replace) =
            (&self.replace, &self.session, self.mode)
        else {
            model.backspace();
            return;
        };
        sync(model);
        let text = model.text().clone();
        let heads = sorted_heads(model);
        let records = replace.at_carets(&text, &heads);
        // Each act's record, and what it does to it.
        let mut undone: Vec<Option<(usize, Undo)>> = Vec::with_capacity(heads.len());
        let acts = {
            let record = replace.record();
            heads
                .iter()
                .zip(&records)
                .map(|(&head, &ix)| {
                    let Some(ix) = ix else {
                        undone.push(None);
                        return Act::stay(head);
                    };
                    let typed = &record.typed[ix];
                    let last = typed.len() - 1;
                    let entry = &typed[last];
                    let made = entry.made.clone();
                    let (range, put, undo) = match (entry.is_break(), entry.put.len()) {
                        // Keys alone: none of their text is left to take.
                        (false, 0) => {
                            undone.push(None);
                            return Act::stay(head);
                        }
                        (false, _) if entry.tab_rest => {
                            // Back through the spaces of the typed tab.
                            let mut first = last;
                            while typed[first].tab == 0 {
                                let Some(below) = first.checked_sub(1).map(|ix| &typed[ix]) else {
                                    break;
                                };
                                if below.made.end != typed[first].made.start || !below.live(&text) {
                                    break;
                                }
                                first -= 1;
                            }
                            let was: String = typed[first..]
                                .iter()
                                .map(|entry| entry.was.as_str())
                                .collect();
                            let range = typed[first].made.start..head;
                            (range, was, Undo::Take { first, keep: None })
                        }
                        (false, _) => {
                            let (line, at) = rest_of_row(&text, head);
                            let grapheme = past(&line, at.saturating_sub(1), 0);
                            // The session's characters just before this one,
                            // and the indentation of a line break before them.
                            let (mut first, mut start) = (last, made.start);
                            while let Some(below) = first.checked_sub(1).map(|ix| &typed[ix]) {
                                if start <= grapheme
                                    || below.made.end != start
                                    || !below.live(&text)
                                {
                                    break;
                                }
                                if below.is_break() {
                                    start = grapheme.max(below.made.start + 1);
                                    break;
                                }
                                start = below.made.start;
                                first -= 1;
                            }
                            let (range, first) = match start <= grapheme {
                                true => (grapheme..head, first),
                                false => (made, last),
                            };
                            // What's left of a line break just before them.
                            let below = first.checked_sub(1).filter(|&ix| {
                                let below = &typed[ix];
                                below.is_break()
                                    && below.made.end == typed[first].made.start
                                    && below.live(&text)
                            });
                            let keep = below.map(|ix| range.start - typed[ix].made.start);
                            (range, entry.was.clone(), Undo::Take { first, keep })
                        }
                        (true, 1) => (
                            made,
                            entry.was.clone(),
                            Undo::Take {
                                first: last,
                                keep: None,
                            },
                        ),
                        (true, _) => (
                            made.start + 1..made.end,
                            entry.indent_was.clone(),
                            Undo::Trim,
                        ),
                    };
                    // The spaces a wide grapheme left go with the character.
                    let range = match entry.trail {
                        0 => range,
                        trail => {
                            let end = head + trail;
                            let spaces = end <= text.len()
                                && text.slice_to_string(head..end) == " ".repeat(trail);
                            range.start..range.end + if spaces { trail } else { 0 }
                        }
                    };
                    undone.push(Some((ix, undo)));
                    Act {
                        range,
                        text: put,
                        land: 0,
                    }
                })
                .collect()
        };
        let starts = act(model, session.origin.clone(), acts);
        let mut record = replace.record();
        for (start, undone) in starts.into_iter().zip(undone) {
            let (Some(_), Some((ix, undo))) = (start, undone) else {
                continue;
            };
            undo.apply(&mut record.typed[ix]);
        }
        record.follow(None);
    }
}

/// What a `<BS>` does to the entries of the record it takes back from, in
/// the text before its edit.
enum Undo {
    /// The line break that's the last loses its indentation.
    Trim,
    /// The entries from `first` on go. With `keep`, the line break before
    /// them keeps that many bytes and their keys.
    Take { first: usize, keep: Option<usize> },
}

impl Undo {
    fn apply(self, typed: &mut Vec<Entry>) {
        let (first, keep) = match self {
            Undo::Trim => {
                if let Some(entry) = typed.last_mut() {
                    entry.put.truncate(1);
                    entry.indent_was.clear();
                    entry.trail = 0;
                    entry.then.push(BACKSPACE);
                }
                return;
            }
            Undo::Take { first, keep } => (first, keep),
        };
        let taken: Vec<Entry> = typed.drain(first..).collect();
        let mut keys = String::new();
        keys_of(&taken, &mut keys);
        keys.push(BACKSPACE);
        let at = taken.first().map_or(0, |entry| entry.made.start);
        match (keep, typed.last_mut()) {
            (Some(keep), Some(below)) => {
                below.put.truncate(keep);
                below.made.end = below.made.start + keep;
                below.then.push_str(&keys);
            }
            // Their keys typed again leave nothing, unless a line break is
            // among them (where a row's indentation differs, `<BS>` may take
            // that instead) or they're the first: the count's copies type
            // them after the line break the last copy ended with, whose
            // indentation they then use, as typing after a line break does.
            (_, Some(_))
                if !taken
                    .iter()
                    .any(|entry| entry.is_break() || entry.leaves_keys()) => {}
            (_, Some(below)) => below.then.push_str(&keys),
            (_, None) => typed.push(Entry {
                made: at..at,
                put: String::new(),
                was: String::new(),
                then: keys,
                touched: false,
                trail: 0,
                indent_was: String::new(),
                tab: 0,
                tab_rest: false,
            }),
        }
    }
}

/// The display width of `text` at `column`, without building a row for the
/// printable ASCII that most of it is.
fn columns_of(text: &str, column: usize, tab: usize) -> usize {
    match text.as_bytes() {
        [b' '..=b'~'] => 1,
        _ => width_from(text, column, tab),
    }
}

/// What one character typed in a virtual session did to the row after the
/// caret, for a `<BS>` to undo.
struct Step {
    at: usize,
    tab_rest: bool,
    width: usize,
    taken: Vec<(String, bool)>,
    trail: usize,
}

/// What typing in a virtual session does at one caret.
struct Virtual {
    /// What goes in: the characters, and the spaces left of a wide
    /// grapheme their last only partly covered.
    text: String,
    /// How far into `text` the caret lands.
    land: usize,
    /// How many of the row's graphemes from the caret it replaces.
    over: usize,
    entries: Vec<Entry>,
}

/// Types `row` (no line break) in a virtual session at display `column`,
/// before grapheme `ix` of `line`, as nvim's `gR` does: each character
/// replaces the graphemes covering the columns it takes, a tab as the
/// spaces to its next stop. A tab only partly covered stays, as it still
/// reaches its stop; a wide grapheme partly covered leaves spaces for its
/// other columns; past the row's end characters go in.
fn virtual_over(
    line: &Line,
    ix: usize,
    mut column: usize,
    row: &str,
    copies: usize,
    tab: usize,
    history: bool,
) -> Virtual {
    // The graphemes after the caret, whether they're the row's own (the
    // ones that leave the row), the leftover spaces being the others.
    let mut rest: VecDeque<(String, bool)> = (ix..line.len())
        .map(|ix| (line.grapheme(ix).to_string(), true))
        .collect();
    let (mut text, mut over) = (String::new(), 0);
    let mut entries: Vec<Entry> = Vec::new();
    // What each character took, for a `<BS>` in the keys to put back.
    let mut steps: Vec<Step> = Vec::new();
    let mut keys = String::new();
    let track = history || row.contains(BACKSPACE);
    for key in std::iter::repeat_n(row, copies).flat_map(|row| row.chars().chain(['\u{0}'])) {
        if key == '\u{0}' {
            // A copy's `<BS>`s take back only what it typed.
            if !history {
                steps.clear();
            }
            continue;
        }
        let spaces = match key {
            BACKSPACE => {
                // The grapheme before the caret goes, and the whole of a
                // typed tab.
                let start = text
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(0, |(at, _)| at);
                // Only the grapheme's last character gives back what it took,
                // as `<BS>` does; a typed tab's spaces all do.
                let mut restore = true;
                while let Some(step) = steps.pop() {
                    for _ in 0..step.trail {
                        rest.pop_front();
                    }
                    if restore {
                        over -= step.taken.iter().filter(|(_, own)| *own).count();
                        for grapheme in step.taken.into_iter().rev() {
                            rest.push_front(grapheme);
                        }
                    }
                    restore = step.tab_rest;
                    text.truncate(step.at);
                    if history {
                        entries.pop();
                    }
                    column -= step.width;
                    // On while inside a typed tab, or the grapheme goes on.
                    if !step.tab_rest && step.at <= start {
                        break;
                    }
                }
                continue;
            }
            '\t' => tab - column % tab,
            _ => 0,
        };
        keys.clear();
        match spaces {
            0 => keys.push(key),
            n => keys.extend(std::iter::repeat_n(' ', n)),
        }
        for (nth, ch) in keys.chars().enumerate() {
            let width = columns_of(ch.encode_utf8(&mut [0; 4]), column, tab);
            let (mut need, mut was, mut trail) = (width, String::new(), 0);
            let mut taken = Vec::new();
            while need > 0 {
                let Some((grapheme, _)) = rest.front() else {
                    break;
                };
                let covered = width - need;
                let columns = columns_of(grapheme, column + covered, tab);
                if columns > need && grapheme == "\t" {
                    break;
                }
                let Some(next) = rest.pop_front() else {
                    break;
                };
                // The spaces left of a wide grapheme aren't the row's.
                if history && next.1 {
                    was.push_str(&next.0);
                }
                over += usize::from(next.1);
                if track {
                    taken.push(next);
                }
                if columns > need {
                    trail = columns - need;
                    for _ in 0..trail {
                        rest.push_front((" ".to_string(), false));
                    }
                    break;
                }
                need -= columns;
            }
            let at = text.len();
            text.push(ch);
            if history {
                let mut entry = Entry::new(at..text.len(), ch.to_string(), was);
                entry.trail = trail;
                // A typed tab is one key: `.` types it again where it lands.
                if spaces > 0 {
                    match nth {
                        0 => entry.tab = spaces,
                        _ => entry.tab_rest = true,
                    }
                }
                entries.push(entry);
            }
            if track {
                steps.push(Step {
                    at,
                    tab_rest: spaces > 0 && nth > 0,
                    width,
                    taken,
                    trail,
                });
            }
            column += width;
        }
    }
    let land = text.len();
    text.extend(rest.iter().take_while(|(_, own)| !own).map(|_| ' '));
    Virtual {
        text,
        land,
        over,
        entries,
    }
}

/// `Esc` in replace mode, for a count: at each caret (`keys`: where it is
/// and the keys typed there, see [`Replace::keys`]) those keys are typed
/// again `count - 1` more times, overwriting, the caret after them. One
/// edit a caret: what the copies overwrite runs to the caret's row's end at
/// most, which a line break moves along ([`copies`]). At most [`MAX_PUT`]
/// bytes go in.
pub(crate) fn repeat_typed(
    model: &mut EditorModel,
    session: &Session,
    count: u32,
    keys: &[(usize, String)],
) {
    let more = count as usize - 1;
    if more == 0 {
        return;
    }
    sync(model);
    let text = model.text().clone();
    // Characters the copies type on the caret's row and take back again
    // leave nothing there to say so: its indentation is used all the same.
    for (head, keys) in keys {
        if keys.split('\n').next().is_some_and(|row| !row.is_empty()) {
            session.typed_on_row(&text, *head);
        }
    }
    let budget = MAX_PUT / keys.len().max(1);
    let virtual_ = matches!(session.change, Change::VirtualReplace(_));
    let tab = model.tab_size().max(1);
    let acts = sorted_heads(model)
        .iter()
        .map(|&head| {
            let typed = keys
                .iter()
                .find(|&&(at, _)| at == head)
                .map_or("", |(_, typed)| typed.as_str());
            if typed.is_empty() {
                return Act::stay(head);
            }
            if virtual_ && typed.contains('\n') && !typed.contains(BACKSPACE) {
                return virtual_copies(&text, head, typed, more, tab, budget);
            }
            let (line, ix) = rest_of_row(&text, head);
            if virtual_ && !typed.contains('\n') {
                let column = width_from(&line.text[..head - line.start], 0, tab);
                // Each tab is up to a tab stop's worth of spaces.
                let size: usize = typed
                    .chars()
                    .map(|ch| if ch == '\t' { tab } else { ch.len_utf8() })
                    .sum();
                let copies = more.min(MAX_PUT / keys.len().max(1) / size.max(1));
                let typed = virtual_over(&line, ix, column, typed, copies, tab, false);
                return Act {
                    range: head..past(&line, ix, typed.over),
                    text: typed.text,
                    land: typed.land,
                };
            }
            let rest: Vec<&str> = (ix..line.len()).map(|ix| line.grapheme(ix)).collect();
            let before = &line.text[..head - line.start];
            let (copied, over) = copies(before, &rest, typed, more, budget);
            Act {
                range: head..past(&line, ix, over),
                land: copied.len(),
                text: copied,
            }
        })
        .collect::<Vec<Act>>();
    #[cfg(test)]
    COPIES_EDITED.set(acts.iter().filter(|act| !act.text.is_empty()).count());
    if let (_, Some(version)) = act_on(model, session.origin.clone(), acts) {
        session.ending(version);
    }
}

/// `more` copies of `keys` (characters and line breaks) typed in a virtual
/// session after the caret at `head`, as [`VimState::line_break`] and
/// [`VimState::overwrite`] type them: each line break deletes the rest of its
/// row and goes on to the next, whose own columns the indentation replaces,
/// and adds a row past the last. One edit, from `head` to the end of the
/// last row of the text they typed over, at most `budget` bytes of typing.
/// Once a copy starts where the one before did, on a row it added, the rest
/// type the same again at once.
fn virtual_copies(
    text: &Text,
    head: usize,
    keys: &str,
    more: usize,
    tab: usize,
    budget: usize,
) -> Act {
    let first = text.offset_to_point(head).row;
    let last = text.row_count().saturating_sub(1);
    let mut cur = text.row_to_string(first);
    let from = head - text.row_start(first);
    let mut at = from;
    let (mut row, mut end) = (first, row_end(text, first));
    let mut out = String::new();
    // The indentation a line break just typed, with nothing after it.
    let mut unused = 0;
    // Where the last copy started, to see the next starts there too.
    let mut began: Option<((String, usize, usize), usize)> = None;
    let mut done = 0;
    while done < more && out.len() <= budget {
        if row > last {
            let state = (cur.clone(), at, unused);
            if let Some((was, len)) = began.replace((state.clone(), out.len()))
                && was == state
            {
                let piece = out[len..].to_string();
                let room = budget.saturating_sub(out.len()) / piece.len().max(1);
                out.push_str(&piece.repeat((more - done).min(room)));
                break;
            }
        }
        for (nth, piece) in keys.split('\n').enumerate() {
            if nth > 0 {
                let before = &cur[..at];
                let indent = before[..before.len() - before.trim_start().len()].to_string();
                cur.truncate(at - unused.min(at));
                out.push_str(&cur);
                out.push('\n');
                row += 1;
                cur = match row <= last {
                    true => {
                        end = row_end(text, row);
                        text.row_to_string(row)
                    }
                    false => String::new(),
                };
                let line = Line::of(0, cur);
                let typed = virtual_over(&line, 0, 0, &indent, 1, tab, false);
                cur = typed.text + &line.text[past(&line, 0, typed.over)..];
                at = typed.land;
                unused = typed.land;
            }
            if piece.is_empty() {
                continue;
            }
            let line = Line::of(0, std::mem::take(&mut cur));
            let ix = line.starts.partition_point(|&start| start < at);
            let column = width_from(&line.text[..at], 0, tab);
            let typed = virtual_over(&line, ix, column, piece, 1, tab, false);
            cur = format!(
                "{}{}{}",
                &line.text[..at],
                typed.text,
                &line.text[past(&line, ix, typed.over)..]
            );
            at += typed.land;
            unused = 0;
        }
        done += 1;
    }
    // Indentation left on a row it added with nothing after it goes, as at
    // `Esc`.
    if row > last {
        cur.truncate(at - unused.min(at));
        at -= unused.min(at);
    }
    let land = out.len() + at - from;
    out.push_str(&cur);
    Act {
        range: head..end,
        text: out.split_off(from.min(out.len())),
        land,
    }
}

/// Types the copy that ended `out` (it cut `cut` bytes, all of it once no
/// more than that, then added `piece`) again, up to `left` more times while
/// `out` stays within `budget` bytes, and returns how many: as many as fit
/// if it grows `out`, all of them if it leaves it as it is, and as many as
/// it can if it shrinks it (while there's `cut` to cut). Each is no work.
pub(crate) fn again(
    out: &mut String,
    piece: &str,
    cut: usize,
    budget: usize,
    left: usize,
) -> usize {
    let len = out.len();
    let keep = len - piece.len();
    out.truncate(keep);
    let more = match piece.len().checked_sub(cut) {
        Some(0) => left,
        Some(grows) => {
            let more = (budget.saturating_sub(len) / grows).min(left);
            out.push_str(&piece[..grows].repeat(more));
            more
        }
        None => {
            let shrinks = cut - piece.len();
            let more = len.checked_sub(cut).map_or(0, |room| room / shrinks + 1);
            let more = more.min(left);
            out.truncate(keep - more * shrinks);
            more
        }
    };
    out.push_str(piece);
    more
}

/// Takes `len` bytes off the end of what's typed, `piece`, and past it,
/// `cut` more off the text before it.
fn take(cut: &mut usize, piece: &mut String, len: usize) {
    match len <= piece.len() {
        true => piece.truncate(piece.len() - len),
        false => {
            *cut += len - piece.len();
            piece.clear();
        }
    }
}

/// Where typing the keys is up to, on the row the caret is on.
#[derive(Clone, PartialEq, Eq)]
struct Typing {
    /// The white space the row starts with, up to the caret, and whether
    /// anything else follows it there.
    lead: String,
    led: bool,
    /// How many bytes at the end of what's typed a line break copied as
    /// indentation with nothing typed after it.
    unused: usize,
    /// How many graphemes of the row's rest (after the caret) are
    /// overwritten or dropped.
    over: usize,
    /// With [`Typing::led`], the last grapheme of the row before this copy
    /// (its last [`CONTEXT`] characters), which the characters typed after
    /// it may join, as a regional indicator pairs with a lone one before
    /// it. A `<BS>` doesn't take it back.
    context: String,
}

type Shape<'a> = (&'a str, bool, usize, &'a str);

/// What a copy typed that a `<BS>` in it takes back, last first.
enum Typed {
    /// A character, whether it overwrote a grapheme, and the row's
    /// [`Typing::lead`] (its length) and [`Typing::led`] before it.
    Char {
        key: char,
        overwrote: bool,
        lead: usize,
        led: bool,
    },
    /// A line break: the row before it as it was, and how many blanks of
    /// the rest it dropped.
    Break {
        lead: String,
        led: bool,
        dropped: usize,
    },
}

impl Typing {
    /// Types `keys` once over `rest`, the row's rest, after text whose last
    /// [`Typing::unused`] bytes are a line break's unused indentation:
    /// returns how many bytes of that text go, and what goes after it. A
    /// `<BS>` takes back only what this copy typed, but for the last blank
    /// of the indentation before it ([`Typing::bite`]).
    fn type_keys(&mut self, mut keys: &str, rest: &[&str]) -> (usize, String) {
        let (mut cut, mut piece) = (0, String::new());
        let mut typed = Vec::new();
        while !keys.is_empty() {
            let run = keys.find(['\n', BACKSPACE]).unwrap_or(keys.len());
            if run > 0 {
                let (run, after) = keys.split_at(run);
                keys = match after.strip_prefix(BACKSPACE) {
                    Some(after) => {
                        self.bite(run, rest, &mut typed, &mut cut, &mut piece);
                        after
                    }
                    None => {
                        for key in run.chars() {
                            typed.push(self.type_char(key, rest, &mut piece));
                        }
                        after
                    }
                };
                continue;
            }
            // A line break or a `<BS>`, a byte each.
            let (key, after) = keys.split_at(1);
            keys = after;
            if key == "\n" {
                // Indentation nothing was typed after goes, as Vim's
                // 'autoindent' takes it out at the next line break.
                let unused = std::mem::take(&mut self.unused);
                take(&mut cut, &mut piece, unused);
                let (lead, led) = match unused {
                    0 => (self.lead.clone(), self.led),
                    _ => (String::new(), false),
                };
                let mut dropped = 0;
                while rest
                    .get(self.over)
                    .is_some_and(|grapheme| grapheme.chars().all(char::is_whitespace))
                {
                    self.over += 1;
                    dropped += 1;
                }
                piece.push('\n');
                piece.push_str(&self.lead);
                self.unused = self.lead.len();
                self.led = false;
                typed.push(Typed::Break { lead, led, dropped });
                continue;
            }
            match typed.last() {
                // Right after a line break, what's left of its indentation
                // goes, then the line break.
                Some(Typed::Break { .. }) if !self.lead.is_empty() => {
                    take(&mut cut, &mut piece, self.lead.len());
                    self.lead.clear();
                    self.unused = 0;
                }
                Some(Typed::Break { .. }) => {
                    if let Some(Typed::Break { lead, led, dropped }) = typed.pop() {
                        take(&mut cut, &mut piece, 1);
                        self.over -= dropped;
                        self.lead = lead;
                        self.led = led;
                    }
                }
                // The grapheme before the caret goes, what its last
                // character overwrote coming back.
                Some(&Typed::Char { overwrote, .. }) => {
                    self.over -= usize::from(overwrote);
                    self.grapheme_back(String::new(), &mut typed, &mut cut, &mut piece);
                }
                None => {}
            }
        }
        self.context = match self.led {
            true => last_grapheme(&self.row(&typed)),
            false => String::new(),
        };
        (cut, piece)
    }

    /// Types `key`, a character, overwriting.
    fn type_char(&mut self, key: char, rest: &[&str], piece: &mut String) -> Typed {
        let typed = Typed::Char {
            key,
            overwrote: self.over < rest.len(),
            lead: self.lead.len(),
            led: self.led,
        };
        self.unused = 0;
        match !self.led && key.is_whitespace() {
            true => self.lead.push(key),
            false => self.led = true,
        }
        piece.push(key);
        if self.over < rest.len() {
            self.over += 1;
        }
        typed
    }

    /// Types `chars`, then a `<BS>` that takes them back (see
    /// [`Entry::then`]): the grapheme they end goes ([`Typing::grapheme_back`])
    /// and what the last of them overwrote comes back (but not what the
    /// others did). Typed on, the indentation stays at a line break or the
    /// end.
    fn bite(
        &mut self,
        chars: &str,
        rest: &[&str],
        typed: &mut Vec<Typed>,
        cut: &mut usize,
        piece: &mut String,
    ) {
        let count = chars.chars().count();
        let overwrote = self.over + count <= rest.len();
        self.over = (self.over + count).min(rest.len()) - usize::from(overwrote);
        self.grapheme_back(chars.to_string(), typed, cut, piece);
        self.unused = 0;
    }

    /// Takes out the grapheme before the caret that ends with `tail`
    /// (characters no longer there), as the session's `<BS>` does: what of
    /// it this copy typed, at least one character if `tail` is empty, and
    /// the last blank of the indentation before that if the grapheme holds
    /// it (right after a line break, this copy's or the one before's). The
    /// grapheme is found in the row up to the caret as far as this copy
    /// knows it ([`Typing::row`]).
    fn grapheme_back(
        &mut self,
        tail: String,
        typed: &mut Vec<Typed>,
        cut: &mut usize,
        piece: &mut String,
    ) {
        let mut row = self.row(typed);
        let known = row.len();
        row.push_str(&tail);
        let start = row
            .grapheme_indices(true)
            .next_back()
            .map_or(known, |(start, _)| start)
            .min(known.saturating_sub(usize::from(tail.is_empty())));
        while let Some(&Typed::Char { key, lead, led, .. }) = typed.last() {
            if row.len() - tail.len() <= start {
                break;
            }
            take(cut, piece, key.len_utf8());
            row.truncate(row.len() - tail.len() - key.len_utf8());
            row.push_str(&tail);
            typed.pop();
            self.lead.truncate(lead);
            self.led = led;
        }
        if start < row.len() - tail.len() && !self.led {
            let blanks = row.len() - tail.len() - start;
            take(cut, piece, blanks);
            self.lead.truncate(self.lead.len() - blanks);
        }
    }

    /// The row up to the caret as far as this copy knows it: the
    /// indentation before the characters typed since its last line break
    /// (all of it if only white space is before them, the copy's
    /// [`Typing::context`] if more is), then those characters.
    fn row(&self, typed: &[Typed]) -> String {
        let below = typed.len()
            - typed
                .iter()
                .rev()
                .take_while(|typed| matches!(typed, Typed::Char { .. }))
                .count();
        let (lead, led) = match typed.get(below) {
            Some(&Typed::Char { lead, led, .. }) => (lead, led),
            _ => (self.lead.len(), self.led),
        };
        // Only the copy's start has more than white space before it on its
        // row: a line break's indentation is all blank.
        let mut row = match led {
            true => self.context.clone(),
            false => self.lead[..lead].to_string(),
        };
        for typed in &typed[below..] {
            if let Typed::Char { key, .. } = typed {
                row.push(*key);
            }
        }
        row
    }

    /// What decides the next copy, but for how far into the rest it is.
    fn shape(&self) -> Shape<'_> {
        (&self.lead, self.led, self.unused, &self.context)
    }
}

/// The last grapheme of `row`, its last [`CONTEXT`] characters.
fn last_grapheme(row: &str) -> String {
    let last = row.graphemes(true).next_back().unwrap_or("");
    let skip = last.chars().count().saturating_sub(CONTEXT);
    last.chars().skip(skip).collect()
}

/// Whether the last copies (see [`copies`]) repeat: the last one or two
/// typed what the one or two before them typed, ending the same, and leave
/// the rest as it was or used up. If so, how many copies repeat and what
/// they cut and typed between them.
fn steady(last: &[(usize, String, Typing)], rest: usize) -> Option<(usize, usize, String)> {
    // Two when a copy leaves a regional indicator unpaired, for the next
    // to pair.
    (1..=2).find_map(|period| {
        let from = last.len().checked_sub(2 * period)?;
        let [before, after] = [from, from + period].map(|at| {
            let (mut cut, mut piece) = (0, String::new());
            for (more, typed, _) in &last[at..at + period] {
                take(&mut cut, &mut piece, *more);
                piece.push_str(typed);
            }
            (cut, piece, &last[at + period - 1].2)
        });
        let settled = after.2.over == before.2.over || after.2.over == rest;
        let steady =
            (before.0, &before.1, before.2.shape()) == (after.0, &after.1, after.2.shape());
        (settled && steady).then_some((period, after.0, after.1))
    })
}

/// `count` copies of `keys` typed after `before` (the caret's row up to it)
/// over `rest` (the row's graphemes after it), at most `budget` bytes: the
/// text they put in and how many graphemes of `rest` it replaces. Each
/// character overwrites a grapheme; a line break copies the row's
/// indentation up to the caret, which goes again at the next line break or
/// at the end if nothing follows it, and drops the blanks it moves to the
/// new row; a [`BACKSPACE`] after characters takes them back
/// ([`Typing::bite`]), and after a line break takes out what's left of its
/// indentation, or with none the line break itself. A copy's characters
/// join the grapheme before it on the row as they would in the buffer
/// ([`Typing::context`]). Once a copy (or two, see [`steady`]) types what
/// the one before typed, ending the same, and leaves the rest as it was or
/// used up, the rest of the count go in at once ([`again`]), whether they
/// grow the text, leave it as it is or shrink it, so a large count takes no
/// longer than a small one.
pub(crate) fn copies(
    before: &str,
    rest: &[&str],
    keys: &str,
    count: usize,
    budget: usize,
) -> (String, usize) {
    if !keys.contains(['\n', BACKSPACE]) {
        let copies = count.min(budget / keys.len().max(1));
        let over = keys.chars().count().saturating_mul(copies).min(rest.len());
        return (keys.repeat(copies), over);
    }
    let lead: String = before.chars().take_while(|ch| ch.is_whitespace()).collect();
    let led = lead.len() < before.len();
    let mut typing = Typing {
        context: match led {
            true => last_grapheme(before),
            false => String::new(),
        },
        led,
        lead,
        unused: 0,
        over: 0,
    };
    let mut out = String::new();
    // The last few copies: what each cut and typed, and where it left off.
    let mut last: Vec<(usize, String, Typing)> = Vec::new();
    let mut done = 0;
    while done < count {
        let mut after = typing.clone();
        let (cut, piece) = after.type_keys(keys, rest);
        // The first copy's line break may cut the session's own
        // indentation, which isn't in `out`: that's left to `Esc`.
        let cut = cut.min(out.len());
        let len = out.len() - cut + piece.len();
        if len > budget {
            break;
        }
        out.truncate(out.len() - cut);
        out.push_str(&piece);
        done += 1;
        if last.len() == 4 {
            last.remove(0);
        }
        last.push((cut, piece, after.clone()));
        typing = after;
        if let Some((copies, cut, piece)) = steady(&last, rest.len()) {
            done += copies * again(&mut out, &piece, cut, budget, (count - done) / copies);
        }
    }
    // Indentation left at the end with nothing after it goes, as at `Esc`.
    if typing.over == rest.len() {
        out.truncate(out.len() - typing.unused.min(out.len()));
    }
    (out, typing.over)
}
