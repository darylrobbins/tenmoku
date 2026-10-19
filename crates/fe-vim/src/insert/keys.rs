//! Insert mode's indent keys: `<C-t>` and `<C-d>` add and take off one
//! shift width of the caret's row's indent (`:help i_CTRL-T`), and `0<C-d>`
//! and `^<C-d>` take all of it (`:help i_0_CTRL-D`, `:help i_^_CTRL-D`).
//!
//! The shift width is the editor's indent width (`EditorModel::indent_style`
//! and `tab_size`, as `>>` has it). As nvim rounds, `<C-t>` goes to the
//! next multiple of it and `<C-d>` to the previous one. The caret stays on
//! the same text. Each key goes in the session as a mark in the typed text:
//! a private-use character in what `.` repeats ([`IndentKey::mark`]), so
//! `.` and a count type the keys again, as nvim's do.
//!
//! `<C-o>` runs one normal-mode command and comes back
//! ([`VimState::one_command`], `:help i_CTRL-O`).
//!
//! `<C-n>` and `<C-p>` complete the keyword before the caret from the
//! buffer's words ([`VimState::complete_next`], `:help i_CTRL-N`).
//!
//! `<C-w>` and `<C-u>` delete the word, and the text typed on the row,
//! before the caret ([`VimState::insert_key`], `:help i_CTRL-W`,
//! `:help i_CTRL-U`). They go in the session as marks too, so `.` and a
//! count type them again, as nvim's do.

use fe_buffer::Origin;
use fe_editor::{EditorModel, IndentStyle, commands::EditorHost};

use super::{Ended, Indent, MAX_PUT, Session, place, type_at_carets};
use crate::{
    LastChange, Mode, VimKey, VimState, VimTarget,
    motion::{Line, class},
    operator::{apply, caret, sync},
    replace::BACKSPACE,
};

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;

#[path = "complete.rs"]
mod complete;
pub(crate) use complete::Completion;
pub use complete::KeywordCompletion;

/// Where `shift` notes a key in the session, if it does.
enum Note {
    No,
    /// In insert mode: with what the primary caret has typed.
    Typed,
    /// In replace mode: with the keys it has typed.
    Keys(String),
}

/// An indent key.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndentKey {
    /// `<C-t>`: to the next multiple of the shift width.
    In,
    /// `<C-d>`: to the previous multiple.
    Out,
    /// `0<C-d>`: all of the indent.
    Zero,
    /// `^<C-d>`: all of the indent, for this row only: the row `<CR>` opens
    /// next has it back.
    Caret,
}

/// `<C-w>` or `<C-u>` in insert or replace mode, which delete before the
/// caret ([`VimState::insert_key`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum DeleteKey {
    /// `<C-w>`: the word before the caret, after any blanks.
    Word,
    /// `<C-u>`: what was typed on the row, or with nothing typed all before
    /// the caret, but for the row's indent while the caret is past it.
    Line,
}

/// A key that goes in a session's history as a mark ([`with_marks`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Indent(IndentKey),
    Delete(DeleteKey),
}

impl Key {
    /// What stands for the key in the text a session typed.
    fn mark(self) -> char {
        match self {
            Self::Indent(key) => key.mark(),
            Self::Delete(DeleteKey::Word) => '\u{e006}',
            Self::Delete(DeleteKey::Line) => '\u{e007}',
        }
    }

    fn of_mark(mark: char) -> Option<Self> {
        let delete = [DeleteKey::Word, DeleteKey::Line].map(Self::Delete);
        IndentKey::ALL
            .map(Self::Indent)
            .into_iter()
            .chain(delete)
            .find(|key| key.mark() == mark)
    }
}

/// A key noted in a session (`Ledger::keys`): what the primary caret had
/// typed before it (in replace mode, the keys), and what was left of that
/// after it, less what a key that deletes took.
#[derive(Clone, Debug)]
pub(crate) struct Noted {
    pub(crate) key: Key,
    pub(crate) before: String,
    pub(crate) after: String,
}

impl Noted {
    /// A key that leaves what was typed as it was.
    fn kept(key: Key, typed: String) -> Self {
        Self {
            key,
            after: typed.clone(),
            before: typed,
        }
    }
}

impl IndentKey {
    const ALL: [Self; 4] = [Self::In, Self::Out, Self::Zero, Self::Caret];

    /// What stands for the key in the text a session typed.
    fn mark(self) -> char {
        match self {
            Self::In => '\u{e000}',
            Self::Out => '\u{e001}',
            Self::Zero => '\u{e002}',
            Self::Caret => '\u{e003}',
        }
    }

    /// Before a literal character that would read as a mark.
    const ESCAPE: char = '\u{e004}';

    /// In insert mode, a `<BS>` that took back a character typed before an
    /// indent key, so `.` and a count type it, the key and the `<BS>` in
    /// that order, as nvim's do. Replace mode's keys have their own
    /// (`replace::BACKSPACE`).
    pub(crate) const BACK: char = '\u{e005}';

    /// Whether `ch`, typed, would read as a mark: a key's
    /// ([`Key::mark`]), [`IndentKey::BACK`] or [`IndentKey::ESCAPE`].
    fn reads_as_mark(ch: char) -> bool {
        ('\u{e000}'..='\u{e007}').contains(&ch)
    }
}

impl VimState {
    /// `<C-t>` in insert or replace mode: the row each caret is on is
    /// indented to the next multiple of the shift width, the caret staying
    /// on its text.
    pub fn indent_in(&self, model: &mut EditorModel) {
        self.indent_key(model, IndentKey::In, false);
    }

    /// `<C-d>` in insert or replace mode: the row each caret is on is
    /// indented to the previous multiple of the shift width. Right after a
    /// typed `0` or `^` (in insert mode) that character goes and all the
    /// indent with it, `^` keeping it for the next row `<CR>` opens
    /// (`:help i_0_CTRL-D`).
    pub fn indent_out(&self, model: &mut EditorModel) {
        if self.mode == Mode::Replace {
            // A `0` or `^` typed over a character: taken back as `<BS>`
            // does, which puts that character back, then all the indent.
            let session = self.session.as_ref();
            let prefix = |head| session.and_then(|session| typed_prefix(model, session, head));
            let heads = heads(model);
            let first = prefix(model.primary_selection().head());
            if let Some(ch) = first.filter(|&ch| heads.iter().all(|&head| prefix(head) == Some(ch)))
            {
                self.replace_backspace(model);
                let key = if ch == '0' {
                    IndentKey::Zero
                } else {
                    IndentKey::Caret
                };
                return self.indent_key(model, key, false);
            }
        }
        self.indent_key(model, IndentKey::Out, self.mode == Mode::Insert);
    }

    fn indent_key(&self, model: &mut EditorModel, key: IndentKey, auto: bool) {
        let Some(session) = self
            .session
            .as_ref()
            .filter(|_| matches!(self.mode, Mode::Insert | Mode::Replace))
        else {
            return;
        };
        let note = match &self.replace {
            Some(replace) if self.mode == Mode::Replace => {
                Note::Keys(replace.keys(model, model.primary_selection().head()).1)
            }
            _ => Note::Typed,
        };
        shift(model, session, key, auto, note);
    }

    /// `<CR>` typed in insert or replace mode: after `^<C-d>`, the row it
    /// opened has the indent the last one lost, if the row it broke has
    /// none now. It's the line break's indentation, which `Esc` drops if
    /// nothing is typed after it, and in replace mode `<BS>` takes out.
    pub fn restore_indent(&self, model: &mut EditorModel) {
        let Some(session) = &self.session else {
            return;
        };
        if session.ledger().restore.is_none() {
            return;
        }
        let replace = self.replace.as_ref().filter(|_| self.mode == Mode::Replace);
        // Found before the indent goes in after them.
        let breaks = replace.map(|replace| replace.breaks_at(model));
        let restored = restore_at(model, session);
        if let (Some(replace), Some(breaks), Some(restored)) = (replace, breaks, restored) {
            replace.indent_breaks(&breaks, &restored);
        }
    }
}

/// [`VimState::restore_indent`] for `session`: the indent `^<C-d>` took off
/// goes back on the row a line break just opened at each caret. Returns
/// what went in at each caret (in order), if anything did.
pub(crate) fn restore_at(
    model: &mut EditorModel,
    session: &Session,
) -> Option<Vec<Option<String>>> {
    let indents = session.ledger().restore.take()?;
    sync(model);
    let text = model.text().clone();
    let heads = heads(model);
    if heads.len() != indents.len() {
        return None;
    }
    let mut edits = Vec::new();
    let mut restored = vec![None; heads.len()];
    let mut last_row = None;
    for (ix, (&head, indent)) in heads.iter().zip(&indents).enumerate() {
        let Some(indent) = indent else {
            continue;
        };
        let row = text.offset_to_point(head).row;
        if last_row.replace(row) == Some(row) || row == 0 {
            continue;
        }
        // Only where the row broken has no indent left, as nvim has it.
        if text.row_to_string(row - 1).starts_with([' ', '\t']) {
            continue;
        }
        let (start, line) = (text.row_start(row), text.row_to_string(row));
        let blanks = line.len() - line.trim_start_matches([' ', '\t']).len();
        if line[..blanks] != **indent {
            edits.push((start..start + blanks, indent.clone()));
            restored[ix] = Some(indent.clone());
        }
    }
    if edits.is_empty() {
        return None;
    }
    let moved: Vec<usize> = heads.iter().map(|&head| through(&edits, head)).collect();
    // The line break's indentation, which `Esc` drops if unused.
    session.ledger().restored.push(model.buffer().version());
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    place(model, &snapshot, &moved);
    Some(restored)
}

/// The `0` or `^` typed just before `head`, if the session's last edit put
/// it there: any key typed since (an indent key among them) took its place.
fn typed_prefix(model: &EditorModel, session: &Session, head: usize) -> Option<char> {
    let text = model.text();
    let before = head.checked_sub(1)?;
    let row = text.offset_to_point(head).row;
    let column = head - text.row_start(row);
    let line = text.row_to_string(row);
    let ch = match line.as_bytes().get(column.checked_sub(1)?)? {
        b'0' => '0',
        b'^' => '^',
        _ => return None,
    };
    let mut ledger = session.ledger();
    ledger.follow(&session.origin);
    let typed = ledger
        .last
        .iter()
        .any(|put| put.start <= before && put.end == head);
    typed.then_some(ch)
}

/// The carets' heads, in order and once each.
fn heads(model: &EditorModel) -> Vec<usize> {
    let mut heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
    heads.sort_unstable();
    heads.dedup();
    heads
}

/// Moves each caret's row's indent as `key` says; `auto`: for `<C-d>`, a
/// caret right after a `0` or `^` typed here takes all its row's indent
/// and that character instead. `note` the key in the
/// session. One batch with the session's origin. Returns how many bytes of
/// indentation it gave the rows it changed: what nvim writes for them,
/// all of each row's new indent.
fn shift(
    model: &mut EditorModel,
    session: &Session,
    key: IndentKey,
    auto: bool,
    note: Note,
) -> usize {
    sync(model);
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let style = model.indent_style();
    let step = match style {
        Some(IndentStyle::Spaces(n)) => usize::from(n).max(1),
        _ => tab,
    };
    let heads = heads(model);
    let primary = model.primary_selection().head();
    // Each caret's own key.
    let keys: Vec<IndentKey> = heads
        .iter()
        .map(|&head| match (auto, typed_prefix(model, session, head)) {
            (true, Some('0')) => IndentKey::Zero,
            (true, Some('^')) => IndentKey::Caret,
            _ => key,
        })
        .collect();
    let drops = |ix: usize| auto && matches!(keys[ix], IndentKey::Zero | IndentKey::Caret);
    let ix = heads.binary_search(&primary).unwrap_or(0);
    match note {
        Note::No => {}
        Note::Typed => {
            let mut typed = primary_typed(model, session);
            // Less the `0` or `^` it takes out.
            if drops(ix) {
                typed.pop();
            }
            session
                .ledger()
                .keys
                .push(Noted::kept(Key::Indent(keys[ix]), typed));
        }
        Note::Keys(typed) => {
            let noted = Noted::kept(Key::Indent(keys[ix]), typed);
            session.ledger().keys.push(noted);
        }
    }
    // The edits, in order and disjoint, as (range, replacement).
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut last_row = None;
    let mut generated = 0;
    if keys.contains(&IndentKey::Caret) {
        session.ledger().restore = Some(
            heads
                .iter()
                .zip(&keys)
                .map(|(&head, &key)| {
                    let line = text.row_to_string(text.offset_to_point(head).row);
                    (key == IndentKey::Caret).then(|| {
                        line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_owned()
                    })
                })
                .collect(),
        );
    }
    for (ix, &head) in heads.iter().enumerate() {
        let key = keys[ix];
        let row = text.offset_to_point(head).row;
        if drops(ix) && head > 0 {
            edits.push((head - 1..head, String::new()));
        }
        if last_row == Some(row) {
            continue;
        }
        last_row = Some(row);
        let line = text.row_to_string(row);
        let old = line.len() - line.trim_start_matches([' ', '\t']).len();
        let column = line.bytes().take(old).fold(0, |column, byte| match byte {
            b'\t' => (column / tab + 1) * tab,
            _ => column + 1,
        });
        let width = match key {
            IndentKey::In => (column / step + 1) * step,
            IndentKey::Out if column == 0 => continue,
            IndentKey::Out => (column - 1) / step * step,
            IndentKey::Zero | IndentKey::Caret => 0,
        };
        let new = match style {
            Some(IndentStyle::Tabs) => "\t".repeat(width / tab) + &" ".repeat(width % tab),
            _ => " ".repeat(width),
        };
        let blanks = &line[..old];
        let start = text.row_start(row);
        if new == blanks {
            continue;
        }
        generated += new.len();
        edits.push(if new.ends_with(blanks) {
            (start..start, new[..new.len() - old].to_owned())
        } else if blanks.starts_with(&new) {
            (start + new.len()..start + old, String::new())
        } else {
            (start..start + old, new)
        });
    }
    if edits.is_empty() {
        return 0;
    }
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let moved: Vec<usize> = heads
        .iter()
        .map(|&head| {
            let floor = text.row_start(text.offset_to_point(head).row);
            through(&edits, head).max(floor.saturating_add_signed(through_shift(&edits, floor)))
        })
        .collect();
    // The blanks put in are indentation, which typing begins past.
    let mut grown = 0isize;
    let mut pieces = Vec::new();
    for (range, new) in &edits {
        if range.is_empty() {
            let start = range.start.saturating_add_signed(grown);
            pieces.push(Indent {
                used: true,
                ..Indent::new(start..start + new.len())
            });
        }
        grown += new.len() as isize - range.len() as isize;
    }
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    {
        let mut ledger = session.ledger();
        ledger.follow(&session.origin);
        ledger.indents.add(pieces);
    }
    place(model, &snapshot, &moved);
    generated
}

/// Where `offset` goes when `edits` (in order, disjoint) are made: moved by
/// what each edit before it, or around it, changes.
fn through(edits: &[(std::ops::Range<usize>, String)], offset: usize) -> usize {
    let mut shift = 0isize;
    for (range, new) in edits {
        let delta = new.len() as isize - range.len() as isize;
        if range.end <= offset || range.start < offset {
            // A caret inside the blanks replaced keeps its column from
            // the text after, as nvim's does: moved by what they changed.
            shift += delta;
        } else {
            break;
        }
    }
    offset.saturating_add_signed(shift)
}

/// How far `edits` move `offset`, a row's start.
fn through_shift(edits: &[(std::ops::Range<usize>, String)], offset: usize) -> isize {
    through(edits, offset) as isize - offset as isize
}

/// What a session typed, in order: `typed`, the text (or in replace mode the
/// keys) it ended with, with the `keys` noted among it, each with what had
/// been typed when it was typed and what it left of that. Where typing
/// after a key took back some of what it left, the history has `back` for
/// each character taken back, then what was typed instead, so typing the
/// history again does what the keys did ([`IndentKey::BACK`] in insert
/// mode, `<BS>` in replace mode's keys). A literal character that reads as
/// a mark is escaped.
pub(crate) fn with_marks(typed: String, keys: &[Noted], back: char) -> String {
    let mut out = String::with_capacity(typed.len() + keys.len() * 3);
    let push = |out: &mut String, text: &str| {
        for ch in text.chars() {
            if IndentKey::reads_as_mark(ch) {
                out.push(IndentKey::ESCAPE);
            }
            out.push(ch);
        }
    };
    let mut from = "";
    for (key, then, after) in keys
        .iter()
        .map(|noted| (Some(noted.key), noted.before.as_str(), noted.after.as_str()))
        .chain([(None, typed.as_str(), typed.as_str())])
    {
        let same = from
            .char_indices()
            .zip(then.chars())
            .find(|&((_, a), b)| a != b)
            .map_or(from.len().min(then.len()), |((at, _), _)| at);
        let same = boundary(then, same);
        for _ in from[same.min(from.len())..].chars() {
            out.push(back);
        }
        push(&mut out, &then[same..]);
        if let Some(key) = key {
            out.push(key.mark());
        }
        from = after;
    }
    out
}

/// A session's history ([`with_marks`]), read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    /// Text typed: in replace mode, keys, `<BS>` among them.
    Text(String),
    /// In insert mode, this many characters before each caret taken back.
    Back(usize),
    Key(Key),
}

/// The pieces of a session's history, `text` ([`with_marks`]).
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut run = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        if ch == IndentKey::ESCAPE && !escaped {
            escaped = true;
            continue;
        }
        let mark = !escaped && IndentKey::reads_as_mark(ch);
        escaped = false;
        if !mark {
            run.push(ch);
            continue;
        }
        if !run.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut run)));
        }
        match (Key::of_mark(ch), pieces.last_mut()) {
            (Some(key), _) => pieces.push(Piece::Key(key)),
            (None, Some(Piece::Back(n))) => *n += 1,
            (None, _) => pieces.push(Piece::Back(1)),
        }
    }
    if !run.is_empty() {
        pieces.push(Piece::Text(run));
    }
    pieces
}

/// The nearest char boundary of `text` at or before `at`.
fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Types `text` at every caret as `.` replays a session: the text, and the
/// keys it holds as marks ([`with_marks`]).
pub(crate) fn retype(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    state: &VimState,
    text: &str,
    origin: Origin,
) {
    let replacing = state.mode == Mode::Replace;
    let type_text = |model: &mut EditorModel, text: &str| match replacing {
        true => crate::replace::retype(state, model, text),
        false => type_at_carets(model, host, text, origin.clone()),
    };
    if state.session.is_none() {
        return type_text(model, text);
    }
    for piece in pieces(text) {
        match piece {
            Piece::Text(run) => type_text(model, &run),
            Piece::Back(n) => {
                back_at_carets(model, n, origin.clone());
            }
            Piece::Key(Key::Indent(key)) => state.indent_key(model, key, false),
            Piece::Key(Key::Delete(key)) => state.delete_key(model, key),
        }
    }
}

/// Takes back the `n` characters before each caret, as `<BS>` takes back
/// typed text, in a batch of its own: never past the start of the text, nor
/// into what the caret before took back. Returns where each caret (of
/// [`heads`] before) went, if any text went.
fn back_at_carets(model: &mut EditorModel, n: usize, origin: Origin) -> Vec<usize> {
    sync(model);
    let text = model.text().clone();
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut floor = 0;
    for head in heads(model) {
        let before = text.slice_to_string(floor.min(head)..head);
        let len: usize = before.chars().rev().take(n).map(char::len_utf8).sum();
        if len > 0 {
            edits.push((head - len..head, String::new()));
        }
        floor = head;
    }
    if edits.is_empty() {
        return Vec::new();
    }
    let moved: Vec<usize> = heads(model)
        .iter()
        .map(|&head| through(&edits, head))
        .collect();
    let (_, snapshot) = apply(model, edits, origin);
    place(model, &snapshot, &moved);
    moved
}

/// `ranks` (sorted heads' ranks) once the carets went where `moved` says
/// ([`back_at_carets`], [`delete_at`]): those that met another are one.
fn follow_ranks(model: &EditorModel, ranks: &mut Vec<usize>, moved: &[usize]) {
    if moved.is_empty() {
        return;
    }
    let heads = heads(model);
    let found = |&rank: &usize| heads.binary_search(moved.get(rank)?).ok();
    let mut next: Vec<usize> = ranks.iter().filter_map(found).collect();
    next.dedup();
    *ranks = next;
}

impl VimState {
    /// A key in insert or replace mode that the host doesn't type itself:
    /// `<C-w>` and `<C-u>` ([`DeleteKey`]). It edits as the open session's
    /// origin, inside its transaction, at every caret. Returns whether vim
    /// took it; any other key is dropped.
    ///
    /// `<C-w>` deletes the word before the caret (the blanks before it
    /// first), a run of word characters or of others of one class, as
    /// nvim's word motions class them. `<C-u>` deletes all before the caret
    /// but the row's indent while the caret is past it. Each stops once at
    /// where the session's typing began: the next goes on past it. At a
    /// row's start either joins the row to the one above. In replace mode
    /// they go back as that many `<BS>`s would ([`VimState::replace_backspace`]),
    /// putting back what typing overwrote, and before where it began only
    /// moving the caret, as nvim's do.
    pub fn insert_key(&self, model: &mut EditorModel, _host: &dyn EditorHost, key: VimKey) -> bool {
        let Some(key) = self.insert_key_of(key) else {
            return false;
        };
        self.delete_key(model, key);
        true
    }

    /// Whether [`VimState::insert_key`] takes `key` now: a host that can't
    /// edit (a read-only view) refuses those, and leaves the others.
    pub fn takes_insert_key(&self, key: VimKey) -> bool {
        self.insert_key_of(key).is_some()
    }

    fn insert_key_of(&self, key: VimKey) -> Option<DeleteKey> {
        let key = match key {
            VimKey::Ctrl('w') => DeleteKey::Word,
            VimKey::Ctrl('u') => DeleteKey::Line,
            _ => return None,
        };
        let typing = matches!(self.mode, Mode::Insert | Mode::Replace) && self.session.is_some();
        typing.then_some(key)
    }

    /// [`VimState::insert_key`]'s `key`, noted in the session in insert mode.
    fn delete_key(&self, model: &mut EditorModel, key: DeleteKey) {
        let Some(session) = self
            .session
            .as_ref()
            .filter(|_| matches!(self.mode, Mode::Insert | Mode::Replace))
        else {
            return;
        };
        if let (Mode::Replace, Some(replace)) = (self.mode, &self.replace) {
            let keys =
                |model: &EditorModel| replace.keys(model, model.primary_selection().head()).1;
            sync(model);
            let before = keys(model);
            // Which `.` and a count take as the key, to go back as far
            // again where they type it; replace mode's keys note each
            // `<BS>` that took something.
            if self.replace_delete(model, session, key) {
                let (key, after) = (Key::Delete(key), keys(model));
                session.ledger().keys.push(Noted { key, before, after });
            }
            return;
        }
        let before = primary_typed(model, session);
        delete_at(model, session, key);
        let after = primary_typed(model, session);
        let key = Key::Delete(key);
        session.ledger().keys.push(Noted { key, before, after });
    }

    /// [`VimState::delete_key`] in replace mode. Each caret on its own goes
    /// back to where what `key` takes starts ([`taken_from`]), as `<BS>`s
    /// would ([`VimState::replace_backspace`]), putting back what typing
    /// overwrote, until it's there; where `<BS>` takes nothing (before
    /// where typing began) it only steps back a grapheme. At a row's start
    /// it goes to the end of the row above, the line break staying, even
    /// one typed, as nvim's does. Returns whether a caret only moved.
    fn replace_delete(&self, model: &mut EditorModel, session: &Session, key: DeleteKey) -> bool {
        sync(model);
        let selections = model.selections();
        let mut order: Vec<usize> = (0..selections.len()).collect();
        order.sort_by_key(|&ix| selections[ix].head());
        let mut ends = vec![0; selections.len()];
        // What the carets before took moves the ones after (all of it is
        // before them), and none goes back into what one before took.
        let (mut shift, mut floor, mut moved) = (0isize, 0, false);
        for &ix in &order {
            let mut head = selections[ix]
                .head()
                .saturating_add_signed(shift)
                .max(floor);
            let text = model.text().clone();
            let row = text.offset_to_point(head).row;
            if head == text.row_start(row) {
                if row > 0 {
                    head -= 1;
                    moved = true;
                }
            } else {
                let from = taken_from(&text, head, typing_start(model, session, head), key);
                let from = from.max(floor);
                while head > from {
                    model.set_selections([caret(head)]);
                    self.replace_backspace(model);
                    sync(model);
                    let now = model.primary_selection().head();
                    head = match now < head {
                        true => now,
                        false => {
                            moved = true;
                            step_back(model.text(), head)
                        }
                    };
                }
            }
            shift += model.text().len() as isize - text.len() as isize;
            ends[ix] = head;
            floor = head;
        }
        model.set_selections(ends.into_iter().map(caret).collect::<Vec<_>>());
        moved
    }
}

/// The start of the grapheme before `head`, which isn't at its row's start.
fn step_back(text: &fe_text::Text, head: usize) -> usize {
    let line = Line::new(text, text.offset_to_point(head).row);
    let column = head - line.start;
    let ix = line.starts.partition_point(|&at| at < column);
    line.start + line.starts[ix.saturating_sub(1)]
}

/// What the primary caret has typed in `session`: from where its typing
/// began, past the indentation the session made there.
fn primary_typed(model: &mut EditorModel, session: &Session) -> String {
    sync(model);
    let primary = model.primary_selection().head();
    let mut ledger = session.ledger();
    ledger.follow(&session.origin);
    session.regions.last().map_or_else(String::new, |region| {
        let start = ledger.indents.past(model.snapshot().resolve(&region.start));
        model.text().slice_to_string(start.min(primary)..primary)
    })
}

/// Where typing began in `session` for the caret at `head`: the start of
/// its region that holds it, past the indentation the session made there.
fn typing_start(model: &EditorModel, session: &Session, head: usize) -> Option<usize> {
    let snapshot = model.snapshot();
    let mut ledger = session.ledger();
    ledger.follow(&session.origin);
    session.regions.iter().find_map(|region| {
        let start = ledger.indents.past(snapshot.resolve(&region.start));
        (start <= head && head <= snapshot.resolve(&region.end)).then_some(start)
    })
}

/// Deletes what `key` takes before each caret ([`taken_from`]), in a batch
/// with the session's origin: never into what the caret before took.
/// Returns where each caret (of [`heads`] before) went, if any text went.
fn delete_at(model: &mut EditorModel, session: &Session, key: DeleteKey) -> Vec<usize> {
    sync(model);
    let text = model.text().clone();
    let heads = heads(model);
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut floor = 0;
    for &head in &heads {
        let from = taken_from(&text, head, typing_start(model, session, head), key);
        let from = from.max(floor);
        if from < head {
            edits.push((from..head, String::new()));
        }
        floor = head;
    }
    if edits.is_empty() {
        return Vec::new();
    }
    let moved: Vec<usize> = heads.iter().map(|&head| through(&edits, head)).collect();
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    place(model, &snapshot, &moved);
    moved
}

/// Where what `key` deletes before `head` starts, as nvim's `ins_bs` has
/// it: at a row's start, the line break before it (none on the first
/// row); otherwise back over graphemes on the row, at least one, and on
/// until the row's start (for `<C-u>`, its indent's end, if `head` is past
/// it), or `stop`, where typing began; `<C-w>` past blanks, then a run of
/// one class.
fn taken_from(text: &fe_text::Text, head: usize, stop: Option<usize>, key: DeleteKey) -> usize {
    let row = text.offset_to_point(head).row;
    let line = Line::new(text, row);
    if head <= line.start {
        return match row {
            0 => head,
            _ => head - 1,
        };
    }
    let column = head - line.start;
    let mut floor = line.start;
    if key == DeleteKey::Line {
        let blanks = line.text.len() - line.text.trim_start_matches([' ', '\t']).len();
        if blanks < column {
            floor += blanks;
        }
    }
    let ix = line.starts.partition_point(|&start| start < column);
    let (mut at, mut word, mut last) = (head, false, None);
    for nth in (0..ix).rev() {
        let start = line.starts[nth];
        let end = line
            .starts
            .get(nth + 1)
            .map_or(line.text.len(), |&end| end.min(column));
        let grapheme = &line.text[start..end];
        if key == DeleteKey::Word {
            let blank = grapheme.starts_with([' ', '\t', '\u{b}', '\u{c}', '\r']);
            let class = class(grapheme, false);
            if !word {
                word = !blank;
            } else if blank || last != Some(class) {
                break;
            }
            last = Some(class);
        }
        at = line.start + start;
        if at <= floor || Some(at) == stop {
            break;
        }
    }
    at
}

/// How many more copies of what a session typed fit in what's left of
/// [`MAX_PUT`] after `spent` bytes, the last copy having grown the text by
/// `last` and the one before by `before`: the next is taken to grow by as
/// much more again, as indentation copied from the row before and indented
/// once more does (`o<C-t>`).
fn fits(spent: usize, last: usize, before: usize) -> bool {
    let next = last.saturating_add(last.saturating_sub(before));
    spent.saturating_add(next) <= MAX_PUT
}

/// `Esc` with a count after typing that held indent keys: what the
/// primary caret typed, in order (`history`, see [`with_marks`]), goes in
/// `count - 1` more times at each caret that typed (`ended`), the keys
/// between, as nvim types them again. Every copy counts against
/// [`MAX_PUT`], what it puts in (`o`'s rows and the indentation they copy
/// too) and all the indentation its keys write: the copies stop before the
/// next would go past it.
pub(crate) fn repeat_marked(
    model: &mut EditorModel,
    session: &Session,
    ended: &[Ended],
    history: &str,
    opens: bool,
) {
    let more = session.count as usize - 1;
    sync(model);
    let mut ranks: Vec<usize> = {
        let heads = heads(model);
        let mut ranks: Vec<usize> = ended
            .iter()
            .filter_map(|region| heads.binary_search(&region.head?).ok())
            .collect();
        ranks.sort_unstable();
        ranks.dedup();
        ranks
    };
    if ranks.is_empty() {
        return;
    }
    let pieces = pieces(history);
    let more = more.min(MAX_PUT / (history.len() + 1) / ranks.len());
    let (mut spent, mut last, mut before) = (0, 0, 0);
    for copy in 0..more {
        if copy > 0 && !fits(spent, last, before) {
            break;
        }
        let (from, mut generated) = (model.text().len(), 0);
        if opens {
            open_row(model, session);
        }
        for piece in &pieces {
            match piece {
                Piece::Text(typed) => {
                    let heads = heads(model);
                    let edits: Vec<_> = ranks
                        .iter()
                        .map(|&rank| (heads[rank]..heads[rank], typed.clone()))
                        .collect();
                    let moved: Vec<usize> = heads
                        .iter()
                        .map(|&head| {
                            head + ranks.iter().filter(|&&rank| heads[rank] <= head).count()
                                * typed.len()
                        })
                        .collect();
                    let (_, snapshot) = apply(model, edits, session.origin.clone());
                    place(model, &snapshot, &moved);
                }
                // Carets these bring together are one from then on.
                Piece::Back(n) => {
                    let moved = back_at_carets(model, *n, session.origin.clone());
                    follow_ranks(model, &mut ranks, &moved);
                }
                Piece::Key(Key::Indent(key)) => {
                    generated += shift(model, session, *key, false, Note::No);
                }
                // Stopping once where the session's typing began, as
                // nvim's copies, typed in the same insert, stop.
                Piece::Key(Key::Delete(key)) => {
                    let moved = delete_at(model, session, *key);
                    follow_ranks(model, &mut ranks, &moved);
                }
            }
            if ranks.is_empty() {
                return;
            }
        }
        let grew = (model.text().len().saturating_sub(from)).saturating_add(generated);
        (spent, before, last) = (spent.saturating_add(grew), last, grew);
    }
}

/// `Esc` in replace mode with a count after typing that held indent keys:
/// the keys the primary caret typed, in order (`history`, see
/// [`with_marks`]), go in `count - 1` more times at each caret that typed
/// (`keys`, see [`crate::replace::Replace::keys`]), overwriting, the indent
/// keys between. Every copy counts against [`MAX_PUT`], what it puts in and
/// all the indentation its keys write: the copies stop before the next
/// would go past it.
pub(crate) fn repeat_replace(
    model: &mut EditorModel,
    session: &Session,
    keys: &[(usize, String)],
    history: &str,
) {
    let more = session.count as usize - 1;
    sync(model);
    let pieces = pieces(history);
    // A `<C-w>` or `<C-u>` that only moved leaves no keys typed.
    let moves = pieces
        .iter()
        .any(|piece| matches!(piece, Piece::Key(Key::Delete(_))));
    let mut ranks: Vec<usize> = {
        let first = heads(model);
        let mut ranks: Vec<usize> = keys
            .iter()
            .filter(|(_, typed)| moves || !typed.is_empty())
            .filter_map(|(head, _)| first.binary_search(head).ok())
            .collect();
        ranks.sort_unstable();
        ranks.dedup();
        ranks
    };
    if ranks.is_empty() {
        return;
    }
    let more = more.min(MAX_PUT / (history.len() + 1) / ranks.len());
    let (mut spent, mut last, mut before) = (0, 0, 0);
    for copy in 0..more {
        if copy > 0 && !fits(spent, last, before) {
            break;
        }
        let (from, mut generated) = (model.text().len(), 0);
        // At each caret, what this copy's characters overwrote, in order
        // (nothing past the row's end). Carets that going back brings
        // together are one from then on, with the first's.
        let mut overwrote: Vec<Vec<String>> = vec![Vec::new(); ranks.len()];
        for piece in &pieces {
            match piece {
                // Each line break on its own, so the indent `^<C-d>` took
                // off goes back after it, as typing `<CR>` puts it back.
                Piece::Text(typed) => {
                    // `<BS>`s after an indent key put back what the
                    // characters before it overwrote.
                    let backs = typed.chars().take_while(|&ch| ch == BACKSPACE).count();
                    let typed = &typed[backs..];
                    put_back(model, session, &mut ranks, &mut overwrote, backs);
                    let plain = !typed.contains(['\n', BACKSPACE]);
                    let before = match plain {
                        true => Some(overwritten(model, &ranks, typed.chars().count())),
                        false => None,
                    };
                    for run in typed.split_inclusive('\n') {
                        sync(model);
                        let heads = heads(model);
                        let typed: Vec<_> = ranks
                            .iter()
                            .map(|&rank| (heads[rank], run.to_owned()))
                            .collect();
                        if !crate::replace::overwrite_plain(model, session, &typed) {
                            crate::replace::repeat_typed(model, session, 2, &typed);
                        }
                        if run.ends_with('\n') {
                            restore_at(model, session);
                        }
                    }
                    match before {
                        Some(before) => {
                            for (stack, taken) in overwrote.iter_mut().zip(before) {
                                stack.extend(taken);
                            }
                        }
                        None => overwrote.iter_mut().for_each(Vec::clear),
                    }
                }
                // Replace mode's history takes characters back with its
                // own `<BS>`, in the text, as it does for `<C-w>` and
                // `<C-u>` where they took something; a mark is one that
                // went back past it.
                Piece::Back(_) => {}
                Piece::Key(Key::Delete(key)) => {
                    back_in_copy(model, session, &mut ranks, &mut overwrote, *key);
                }
                Piece::Key(Key::Indent(key)) => {
                    generated += shift(model, session, *key, false, Note::No);
                }
            }
            if ranks.is_empty() {
                return;
            }
        }
        let grew = (model.text().len().saturating_sub(from)).saturating_add(generated);
        (spent, before, last) = (spent.saturating_add(grew), last, grew);
    }
}

/// The `n` graphemes after the caret at each of `ranks` (sorted heads'
/// ranks) that typing `n` characters there overwrites in replace mode: on
/// its row, nothing past its end.
fn overwritten(model: &mut EditorModel, ranks: &[usize], n: usize) -> Vec<Vec<String>> {
    use unicode_segmentation::UnicodeSegmentation as _;
    sync(model);
    let text = model.text().clone();
    let heads = heads(model);
    ranks
        .iter()
        .map(|&rank| {
            let head = heads[rank];
            let row = text.offset_to_point(head).row;
            // Only what's after the caret: the row may be long with indent.
            let rest = text.slice_to_string(head..crate::operator::row_end(&text, row));
            let mut taken: Vec<String> = rest.graphemes(true).take(n).map(str::to_owned).collect();
            taken.resize(n, String::new());
            taken
        })
        .collect()
}

/// `backs` `<BS>`s at the carets at `ranks`, each putting back the last of
/// what the copy's characters `overwrote` there, as replace mode's `<BS>`
/// does; where there's none left, nothing.
fn put_back(
    model: &mut EditorModel,
    session: &Session,
    ranks: &mut Vec<usize>,
    overwrote: &mut Vec<Vec<String>>,
    backs: usize,
) {
    use unicode_segmentation::UnicodeSegmentation as _;
    for _ in 0..backs {
        sync(model);
        let text = model.text().clone();
        let heads = heads(model);
        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        for (&rank, stack) in ranks.iter().zip(overwrote.iter_mut()) {
            let Some(was) = stack.pop() else {
                continue;
            };
            let head = heads[rank];
            let row_start = text.row_start(text.offset_to_point(head).row);
            // The grapheme before the caret, from a little way back.
            let mut from = head.saturating_sub(64).max(row_start);
            while !text.is_char_boundary(from) {
                from += 1;
            }
            let before = text.slice_to_string(from..head);
            let Some(last) = before.graphemes(true).next_back() else {
                continue;
            };
            edits.push((head - last.len()..head, was));
        }
        if edits.is_empty() {
            return;
        }
        let moved: Vec<usize> = heads
            .iter()
            .map(|&head| {
                let at = edits.iter().find(|(range, _)| range.end == head);
                let start = at.map_or(head, |(range, _)| range.start);
                through(&edits, start)
            })
            .collect();
        let (_, snapshot) = apply(model, edits, session.origin.clone());
        place(model, &snapshot, &moved);
        follow_stacks(model, ranks, overwrote, &moved);
    }
}

/// `ranks` (sorted heads' ranks) and what the carets at them `overwrote`,
/// once the carets (all of [`heads`] before) went where `moved` says: those
/// that met are one, with the first's.
fn follow_stacks(
    model: &EditorModel,
    ranks: &mut Vec<usize>,
    overwrote: &mut Vec<Vec<String>>,
    moved: &[usize],
) {
    let heads = heads(model);
    let (mut next, mut stacks) = (Vec::new(), Vec::new());
    for (rank, stack) in ranks.iter().zip(overwrote.drain(..)) {
        let Some(Ok(now)) = moved.get(*rank).map(|at| heads.binary_search(at)) else {
            continue;
        };
        if next.last() != Some(&now) {
            next.push(now);
            stacks.push(stack);
        }
    }
    (*ranks, *overwrote) = (next, stacks);
}

/// `<C-w>` or `<C-u>` (`key`) in a replace-mode count's copy, at the carets
/// at `ranks` (sorted heads' ranks), each on its own: back to where what it
/// takes starts ([`taken_from`]), putting back what the copy's characters
/// `overwrote` there ([`put_back`]), then only stepping back a grapheme; at
/// a row's start, to the end of the row above. As
/// [`VimState::replace_delete`] goes back.
fn back_in_copy(
    model: &mut EditorModel,
    session: &Session,
    ranks: &mut Vec<usize>,
    overwrote: &mut Vec<Vec<String>>,
    key: DeleteKey,
) {
    sync(model);
    // Every caret's head, as each at `ranks` goes back on its own: what one
    // puts back is all before the carets after it, which it moves.
    let mut all = heads(model);
    for (nth, &rank) in ranks.iter().enumerate() {
        let text = model.text().clone();
        let start = all[rank];
        let floor = rank.checked_sub(1).map_or(0, |before| all[before]);
        let row = text.offset_to_point(start).row;
        if start == text.row_start(row) {
            if row > 0 {
                all[rank] = start - 1;
            }
            continue;
        }
        let from = taken_from(&text, start, typing_start(model, session, start), key).max(floor);
        let mut head = start;
        while head > from {
            model.set_selections([caret(head)]);
            let now = match overwrote[nth].is_empty() {
                true => step_back(model.text(), head),
                false => {
                    let mut stack = vec![std::mem::take(&mut overwrote[nth])];
                    put_back(model, session, &mut vec![0], &mut stack, 1);
                    overwrote[nth] = stack.pop().unwrap_or_default();
                    sync(model);
                    model.primary_selection().head()
                }
            };
            if now >= head {
                break;
            }
            head = now;
        }
        let grew = model.text().len() as isize - text.len() as isize;
        all[rank] = head;
        for later in &mut all[rank + 1..] {
            *later = later.saturating_add_signed(grew);
        }
    }
    model.set_selections(all.iter().map(|&head| caret(head)).collect::<Vec<_>>());
    sync(model);
    follow_stacks(model, ranks, overwrote, &all);
}

/// A row opened below each caret's, as `o` does for a count's copy: with
/// the indent of the caret's row, the caret at its end.
fn open_row(model: &mut EditorModel, session: &Session) {
    sync(model);
    let text = model.text().clone();
    let heads = heads(model);
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut rows = Vec::new();
    for &head in &heads {
        let row = text.offset_to_point(head).row;
        if rows.last() != Some(&row) {
            let line = text.row_to_string(row);
            let blanks = line.len() - line.trim_start_matches([' ', '\t']).len();
            let end = text.row_start(row) + line.len();
            edits.push((end..end, format!("\n{}", &line[..blanks])));
            rows.push(row);
        }
    }
    let mut moved = Vec::new();
    let mut before = 0;
    let mut ix = 0;
    let mut last = None;
    for &head in &heads {
        let row = text.offset_to_point(head).row;
        if last != Some(row) {
            if last.is_some() {
                before += edits[ix].1.len();
                ix += 1;
            }
            last = Some(row);
        }
        let (range, new) = &edits[ix];
        moved.push(range.start + before + new.len());
    }
    let (_, snapshot) = apply(model, edits, session.origin.clone());
    place(model, &snapshot, &moved);
}

/// What `<C-o>` left to come back to ([`VimState::one_command`]): the
/// session it ended resumes once the command after it is done.
#[derive(Clone, Debug)]
pub(crate) struct OneCommand {
    origin: Origin,
    /// In replace mode it resumes as replace mode, virtual for `gR`.
    replace: Option<bool>,
    /// The rows a caret was at the end of: on that row again, a caret on
    /// its row's last grapheme goes back past it (nvim's `ins_at_eol`).
    eol_rows: Vec<u32>,
    /// What `.` repeats once the session resumes, unless the command made
    /// a change of its own (`changes`): the text typed before `<C-o>`.
    /// Meanwhile `.` repeats the change before the session, as nvim's
    /// `<C-o>.` does.
    ended: Option<LastChange>,
    changes: u64,
}

impl VimState {
    /// `<C-o>` in insert or replace mode (`:help i_CTRL-O`): the session
    /// ends as a split ends it ([`VimState::split_session`]), the mode is
    /// normal for one command, and then the session resumes at the carets
    /// it leaves, in the mode it came from. Meanwhile `vim.mode` is
    /// `normal`, with the flag `vim.insert_pending`.
    ///
    /// The carets stay where they are, but one past its row's end steps
    /// onto its last grapheme, keeping its column as the goal for `j` and
    /// `k`, and once the command is done it goes back past it if it's on
    /// that row's last grapheme, or if the goal is past it (after `$`), as
    /// nvim's does. The indentation the session made that went unused goes,
    /// and a session over a block ends as `Esc` ends it, typing on its other
    /// rows. A count before the session is dropped, as nvim drops it.
    ///
    /// The command is the next complete one: a count and a register belong
    /// to it, `<Esc>` with nothing pending ends it at once, one that enters
    /// a visual mode or opens the command line is done once that ends, and
    /// one that enters insert or replace mode (`o`, `cw`) goes on there. A
    /// `@` plays its register, then enters insert mode again as `i` does
    /// (`R` or `gR` from replace mode), as nvim puts the key after the
    /// register's.
    pub fn one_command(&mut self, model: &mut EditorModel) {
        if !matches!(self.mode, Mode::Insert | Mode::Replace) {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let origin = session.origin.clone();
        let block = session.block.is_some();
        let replace = (self.mode == Mode::Replace)
            .then(|| self.replace.as_ref().is_some_and(|r| r.is_virtual()));
        sync(model);
        let tab = model.tab_size().max(1);
        // Each caret's column, which it keeps as its goal, and whether it's
        // past its row's end.
        let carets: Vec<(usize, bool, u32)> = {
            let text = model.text();
            model
                .selections()
                .iter()
                .map(|selection| {
                    let (line, ix, row) = place_of(text, selection.head());
                    (line.start_column(ix, tab), ix == line.len(), row)
                })
                .collect()
        };
        let before = self.last_change.clone();
        let ending = match block {
            true => super::Ending::Esc,
            false => super::Ending::Split,
        };
        super::end(model, self, ending, None);
        let eol_rows = carets
            .iter()
            .filter(|&&(_, eol, _)| eol)
            .map(|&(_, _, row)| row)
            .collect();
        if !block {
            sync(model);
            let text = model.text().clone();
            let selections = model.selections();
            let mut goals = Vec::with_capacity(selections.len());
            let stepped: Vec<_> = selections
                .iter()
                .enumerate()
                .map(|(ix, selection)| {
                    // Clamped onto its row's last grapheme, if it's past it.
                    let head = crate::motion::Cursor::at(&text, selection.head()).offset();
                    goals.push((head, carets.get(ix).map(|&(column, _, _)| column)));
                    caret(head)
                })
                .collect();
            model.set_selections(stepped);
            let heads = model.selections();
            if heads.len() == goals.len() {
                self.goals = Some((model.snapshot().version(), goals));
            }
        }
        let ended = std::mem::replace(&mut self.last_change, before);
        self.one_command = Some(OneCommand {
            origin,
            replace,
            eol_rows,
            ended,
            changes: self.changes,
        });
    }

    /// Whether `<C-o>` left normal mode for one command
    /// ([`VimState::one_command`]): `vim.insert_pending`.
    pub fn insert_pending(&self) -> bool {
        self.one_command.is_some()
    }

    /// Whether the command after `<C-o>` is done, as of the mode and what's
    /// pending: the session it left can resume.
    pub(crate) fn one_command_done(&self) -> bool {
        self.one_command.is_some()
            && self.mode == Mode::Normal
            && self.pending.kind().is_none()
            && self.confirm.is_none()
            && !self.remap.waiting()
            && !self.remap.resolved()
            && !self.macros.held()
    }
}

/// `0` or `^` (`motion`) after `<C-o>`, moving or under an operator: once
/// the command is done, the carets no longer go back past their rows' ends
/// for having been there, as nvim's `nv_beginline` clears `ins_at_eol`.
pub(crate) fn began_line(state: &mut VimState, motion: crate::Motion) {
    if let (Some(one), crate::Motion::LineStart | crate::Motion::FirstNonBlank) =
        (&mut state.one_command, motion)
    {
        one.eol_rows.clear();
    }
}

/// The row `head` is on, the index of the grapheme it's at (the row's
/// length past its end) and the row's number.
fn place_of(text: &fe_text::Text, head: usize) -> (crate::motion::Line, usize, u32) {
    let row = text.offset_to_point(head).row;
    let line = crate::motion::Line::new(text, row);
    let column = head.saturating_sub(line.start);
    let ix = line.starts.partition_point(|&start| start < column);
    (line, ix, row)
}

/// A command run (or a key read) after `<C-o>`: if it's done, the session
/// `<C-o>` left resumes ([`VimState::one_command`]). A command that entered
/// insert or replace mode goes on there; one that played a register enters
/// it again after the register's keys.
pub(crate) fn finish_one_command(target: &mut VimTarget<'_>) {
    let state = &mut *target.state;
    if matches!(state.mode, Mode::Insert | Mode::Replace) {
        state.one_command = None;
        return;
    }
    if !state.one_command_done() {
        return;
    }
    let Some(one) = state.one_command.take() else {
        return;
    };
    if state.changes == one.changes {
        state.last_change = one.ended;
    }
    if state.macros.queued() {
        // As nvim's `@` puts the key that enters insert mode again after
        // the register's keys.
        let keys = match one.replace {
            None => vec![VimKey::Char('i')],
            Some(false) => vec![VimKey::Char('R')],
            Some(true) => vec![VimKey::Char('g'), VimKey::Char('R')],
        };
        state.macros.queue_after(keys);
        return;
    }
    let model = &mut *target.model;
    sync(model);
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let selections = model.selections();
    let goals = crate::motion::valid_goals(state, model.snapshot().version(), &selections)
        .map(|(_, goals)| goals.clone());
    let heads: Vec<_> = selections
        .iter()
        .enumerate()
        .map(|(ix, selection)| {
            let head = selection.head();
            let (line, at, row) = place_of(&text, head);
            let goal = goals.as_ref().and_then(|goals| goals[ix].1);
            let past = one.eol_rows.contains(&row)
                || goal.is_some_and(|goal| goal > line.start_column(at, tab));
            match past && at + 1 == line.len() {
                true => caret(line.start + line.text.len()),
                false => caret(head),
            }
        })
        .collect();
    model.set_selections(heads);
    super::resume(model, state, one.origin, None);
    if let Some(virtual_) = one.replace {
        crate::replace::resumed(state, model, virtual_);
    }
}
