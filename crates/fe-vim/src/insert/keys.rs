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

use fe_buffer::Origin;
use fe_editor::{EditorModel, IndentStyle, commands::EditorHost};

use super::{Ended, Indent, MAX_PUT, Session, place, type_at_carets};
use crate::{
    LastChange, Mode, VimKey, VimState, VimTarget,
    operator::{apply, caret, sync},
    replace::BACKSPACE,
};

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;

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

impl IndentKey {
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

    /// Whether `ch`, typed, would read as a mark.
    fn reads_as_mark(ch: char) -> bool {
        ('\u{e000}'..='\u{e005}').contains(&ch)
    }

    fn of_mark(mark: char) -> Option<Self> {
        [Self::In, Self::Out, Self::Zero, Self::Caret]
            .into_iter()
            .find(|key| key.mark() == mark)
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
            let mut ledger = session.ledger();
            ledger.follow(&session.origin);
            let mut typed = session.regions.last().map_or_else(String::new, |region| {
                let start = ledger.indents.past(model.snapshot().resolve(&region.start));
                text.slice_to_string(start.min(primary)..primary)
            });
            // Less the `0` or `^` it takes out.
            if drops(ix) {
                typed.pop();
            }
            ledger.keys.push((keys[ix], typed));
        }
        Note::Keys(keys_typed) => session.ledger().keys.push((keys[ix], keys_typed)),
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
/// keys) it ended with, with the indent `keys` among it, each with what had
/// been typed when it was typed. Where typing between two keys took back
/// some of what was typed before the first, the history has `back` for
/// each character taken back, then what was typed instead, so typing the
/// history again does what the keys did ([`IndentKey::BACK`] in insert
/// mode, `<BS>` in replace mode's keys). A literal character that reads as
/// a mark is escaped.
pub(crate) fn with_marks(typed: String, keys: &[(IndentKey, String)], back: char) -> String {
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
    for (key, then) in keys
        .iter()
        .map(|(key, then)| (Some(*key), then.as_str()))
        .chain([(None, typed.as_str())])
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
        from = then;
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
    Key(IndentKey),
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
        match (IndentKey::of_mark(ch), pieces.last_mut()) {
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
/// indent keys it holds as marks ([`with_marks`]).
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
            Piece::Back(n) => back_at_carets(model, n, origin.clone()),
            Piece::Key(key) => state.indent_key(model, key, false),
        }
    }
}

/// Takes back the `n` characters before each caret, as `<BS>` takes back
/// typed text, in a batch of its own: never past the start of the text, nor
/// into what the caret before took back.
fn back_at_carets(model: &mut EditorModel, n: usize, origin: Origin) {
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
        return;
    }
    let moved: Vec<usize> = heads(model)
        .iter()
        .map(|&head| through(&edits, head))
        .collect();
    let (_, snapshot) = apply(model, edits, origin);
    place(model, &snapshot, &moved);
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
    let ranks: Vec<usize> = {
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
                Piece::Back(n) => back_at_carets(model, *n, session.origin.clone()),
                Piece::Key(key) => generated += shift(model, session, *key, false, Note::No),
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
    let ranks: Vec<usize> = {
        let first = heads(model);
        let mut ranks: Vec<usize> = keys
            .iter()
            .filter(|(_, typed)| !typed.is_empty())
            .filter_map(|(head, _)| first.binary_search(head).ok())
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
        // At each caret, what this copy's characters overwrote, in order
        // (nothing past the row's end).
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
                    put_back(model, session, &ranks, &mut overwrote, backs);
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
                // own `<BS>`, in the text.
                Piece::Back(_) => {}
                Piece::Key(key) => generated += shift(model, session, *key, false, Note::No),
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
    ranks: &[usize],
    overwrote: &mut [Vec<String>],
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
    }
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
