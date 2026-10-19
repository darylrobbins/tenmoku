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

use fe_buffer::Origin;
use fe_editor::{EditorModel, IndentStyle, commands::EditorHost};

use super::{Ended, Indent, MAX_PUT, Session, place, type_at_carets};
use crate::{
    Mode, VimState,
    operator::{apply, sync},
};

/// Where `shift` notes a key in the session, if it does.
enum Note {
    No,
    /// In insert mode: how much the primary caret has typed.
    Typed,
    /// In replace mode: how long the keys it has typed are.
    Keys(usize),
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
            Some(replace) if self.mode == Mode::Replace => Note::Keys(
                replace
                    .keys(model, model.primary_selection().head())
                    .1
                    .len(),
            ),
            _ => Note::Typed,
        };
        shift(model, session, key, auto, note);
    }

    /// `<CR>` typed in insert or replace mode: after `^<C-d>`, the row it
    /// opened has the indent the last one lost, if the row it broke has
    /// none now.
    pub fn restore_indent(&self, model: &mut EditorModel) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(indents) = session.ledger().restore.take() else {
            return;
        };
        sync(model);
        let text = model.text().clone();
        let heads = heads(model);
        if heads.len() != indents.len() {
            return;
        }
        let mut edits = Vec::new();
        let mut last_row = None;
        for (&head, indent) in heads.iter().zip(&indents) {
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
            }
        }
        if edits.is_empty() {
            return;
        }
        let moved: Vec<usize> = heads.iter().map(|&head| through(&edits, head)).collect();
        let (_, snapshot) = apply(model, edits, session.origin.clone());
        place(model, &snapshot, &moved);
    }
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
/// session. One batch with the session's origin.
fn shift(model: &mut EditorModel, session: &Session, key: IndentKey, auto: bool, note: Note) {
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
            let typed = session.regions.last().map_or(0, |region| {
                let start = model.snapshot().resolve(&region.start);
                primary.saturating_sub(ledger.indents.past(start))
            });
            ledger
                .keys
                .push((typed.saturating_sub(drops(ix).into()), keys[ix]));
        }
        Note::Keys(at) => session.ledger().keys.push((at, keys[ix])),
    }
    // The edits, in order and disjoint, as (range, replacement).
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut last_row = None;
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
        edits.push(if new.ends_with(blanks) {
            (start..start, new[..new.len() - old].to_owned())
        } else if blanks.starts_with(&new) {
            (start + new.len()..start + old, String::new())
        } else {
            (start..start + old, new)
        });
    }
    if edits.is_empty() {
        return;
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

/// `typed` with the session's `keys` among it, each where it was typed; a
/// literal character that reads as a mark is escaped.
pub(crate) fn with_marks(typed: String, keys: &[(usize, IndentKey)]) -> String {
    let literal = |ch: char| ('\u{e000}'..='\u{e004}').contains(&ch);
    let mut out = String::with_capacity(typed.len() + keys.len() * 3);
    let push = |out: &mut String, text: &str| {
        for ch in text.chars() {
            if literal(ch) {
                out.push(IndentKey::ESCAPE);
            }
            out.push(ch);
        }
    };
    let mut from = 0;
    for &(at, key) in keys {
        let at = boundary(&typed, at).max(from);
        push(&mut out, &typed[from..at]);
        out.push(key.mark());
        from = at;
    }
    push(&mut out, &typed[from..]);
    out
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
    let mut run = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        let key = IndentKey::of_mark(ch).filter(|_| !escaped);
        if ch == IndentKey::ESCAPE && !escaped {
            escaped = true;
            continue;
        }
        escaped = false;
        let Some(key) = key else {
            run.push(ch);
            continue;
        };
        if !run.is_empty() {
            type_text(model, &std::mem::take(&mut run));
        }
        state.indent_key(model, key, false);
    }
    if !run.is_empty() {
        type_text(model, &run);
    }
}

/// `Esc` with a count after typing that held indent keys (`keys`, where
/// each was typed in the text): the text typed at each caret (`ended`) goes
/// in `count - 1` more times, the keys between, as nvim types them again.
pub(crate) fn repeat_marked(
    model: &mut EditorModel,
    session: &Session,
    ended: &[Ended],
    keys: &[(usize, IndentKey)],
    opens: bool,
) {
    let more = session.count as usize - 1;
    sync(model);
    let carets: Vec<(usize, &str)> = {
        let heads = heads(model);
        let mut carets: Vec<(usize, &str)> = ended
            .iter()
            .filter_map(|region| {
                Some((
                    heads.binary_search(&region.head?).ok()?,
                    region.typed.as_str(),
                ))
            })
            .collect();
        carets.sort_by_key(|&(rank, _)| rank);
        carets
    };
    let longest = carets
        .iter()
        .map(|(_, typed)| typed.len())
        .max()
        .unwrap_or(0);
    let more = more.min(MAX_PUT / (longest + keys.len() + 1) / carets.len().max(1));
    for _ in 0..more {
        if opens {
            open_row(model, session);
        }
        let mut from = vec![0; carets.len()];
        for key in keys
            .iter()
            .map(|&(at, key)| (at, Some(key)))
            .chain([(usize::MAX, None)])
        {
            let heads = heads(model);
            let mut edits = Vec::new();
            for (ix, &(rank, typed)) in carets.iter().enumerate() {
                let to = boundary(typed, key.0).max(from[ix]);
                if to > from[ix] {
                    edits.push((heads[rank]..heads[rank], typed[from[ix]..to].to_owned()));
                }
                from[ix] = to;
            }
            if !edits.is_empty() {
                let moved: Vec<usize> = heads
                    .iter()
                    .map(|&head| {
                        head + edits
                            .iter()
                            .filter(|(range, _)| range.start <= head)
                            .map(|(_, text)| text.len())
                            .sum::<usize>()
                    })
                    .collect();
                let (_, snapshot) = apply(model, edits, session.origin.clone());
                place(model, &snapshot, &moved);
            }
            if let Some(key) = key.1 {
                shift(model, session, key, false, Note::No);
            }
        }
    }
}

/// `Esc` in replace mode with a count after typing that held indent keys
/// (`marks`, where each was typed in the primary caret's keys): the keys
/// typed at each caret (`keys`, see [`crate::replace::Replace::keys`]) go in
/// `count - 1` more times, overwriting, the indent keys between.
pub(crate) fn repeat_replace(
    model: &mut EditorModel,
    session: &Session,
    keys: &[(usize, String)],
    marks: &[(usize, IndentKey)],
) {
    let more = session.count as usize - 1;
    sync(model);
    let first = heads(model);
    let carets: Vec<(usize, &str)> = keys
        .iter()
        .filter_map(|(head, typed)| Some((first.binary_search(head).ok()?, typed.as_str())))
        .collect();
    let longest = carets
        .iter()
        .map(|(_, typed)| typed.len())
        .max()
        .unwrap_or(0);
    let more = more.min(MAX_PUT / (longest + marks.len() + 1) / carets.len().max(1));
    for _ in 0..more {
        let mut from = vec![0; carets.len()];
        for (at, key) in marks
            .iter()
            .map(|&(at, key)| (at, Some(key)))
            .chain([(usize::MAX, None)])
        {
            sync(model);
            let heads = heads(model);
            let mut typed = Vec::new();
            for (ix, &(rank, keys)) in carets.iter().enumerate() {
                let to = boundary(keys, at).max(from[ix]);
                if to > from[ix] {
                    typed.push((heads[rank], keys[from[ix]..to].to_owned()));
                }
                from[ix] = to;
            }
            if !typed.is_empty() {
                crate::replace::repeat_typed(model, session, 2, &typed);
            }
            if let Some(key) = key {
                shift(model, session, key, false, Note::No);
            }
        }
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
