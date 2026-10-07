//! `J`, `r` and `~`: `vim.join`, `vim.replace_char` and `vim.toggle_case`.
//! Each is one transaction for every caret, with the actor's origin.

use std::{ops::Range, sync::OnceLock};

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::commands::origin;

use crate::{
    Change, CountArgs, ReplaceCharArgs, VimTarget, history,
    motion::Cursor,
    operator::{self, row_end},
};

/// One caret's edits, sorted, and where it lands; `None` if it doesn't act
/// and stays put.
#[derive(Default)]
struct Acts {
    edits: Vec<(Range<usize>, String)>,
    land: Option<Land>,
}

enum Land {
    /// An offset into the new text of one of its edits.
    Edit(usize, usize),
    /// An offset in the text before (it changed nothing).
    At(usize),
}

/// Applies every caret's [`Acts`] as one transaction and places the carets;
/// records the change if anything was edited. Carets whose edits would
/// overlap an earlier caret's are left out.
fn apply(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    acts: Vec<Acts>,
    change: Change,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    let before = model.selections();
    let carets_before = history::pairs(model);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    // Each caret's landing edit (by index in `edits`) and offset, or an
    // offset in the text before.
    let mut lands: Vec<Result<(usize, usize), usize>> = Vec::with_capacity(acts.len());
    let mut acted = false;
    for (selection, act) in before.iter().zip(acts) {
        let free = act
            .edits
            .first()
            .is_none_or(|(range, _)| edits.last().is_none_or(|(last, _)| last.end <= range.start));
        match act.land {
            Some(Land::Edit(ix, offset)) if free => {
                lands.push(Ok((edits.len() + ix, offset)));
                edits.extend(act.edits);
                acted = true;
            }
            Some(Land::At(offset)) => {
                lands.push(Err(offset));
                acted = true;
            }
            _ => lands.push(Err(selection.head())),
        }
    }
    // Where each edit's new text starts once all are applied.
    let mut shift = 0isize;
    let starts: Vec<usize> = edits
        .iter()
        .map(|(range, text)| {
            let start = range.start.checked_add_signed(shift).expect("sorted edits");
            shift += text.len() as isize - range.len() as isize;
            start
        })
        .collect();
    // An offset once the edits are in: one inside a replaced range stays
    // inside the new text, as far in as it still reaches.
    let mapped = |offset: usize| {
        let mut shift = 0isize;
        for ((range, text), &start) in edits.iter().zip(&starts) {
            if range.end <= offset {
                shift += text.len() as isize - range.len() as isize;
            } else if range.start < offset {
                return start + (offset - range.start).min(text.len());
            } else {
                break;
            }
        }
        offset
            .checked_add_signed(shift)
            .expect("an offset past its edits")
    };
    let heads: Vec<usize> = lands
        .iter()
        .map(|land| match *land {
            Ok((ix, offset)) => starts[ix] + offset,
            Err(head) => mapped(head),
        })
        .collect();
    // The carets land in the text right after the edits, which measuring
    // text may have edited on from (see `operator::apply`).
    let (transaction, snapshot) = match edits.is_empty() {
        true => (None, model.snapshot().clone()),
        false => operator::apply(model, edits, origin(actor)),
    };
    let after = snapshot.text().clone();
    let heads: Vec<usize> = heads
        .into_iter()
        .map(|head| Cursor::at(&after, head).offset())
        .collect();
    operator::land(model, &snapshot, heads);
    if let Some(id) = transaction {
        state.carets.record(id, carets_before);
    }
    if acted {
        state.last_change = Some(change.into());
        state.goals = None;
    }
    let effect = match transaction.is_some() || model.selections() != before {
        true => Effect::Changed,
        false => Effect::Nothing,
    };
    Ok(Dispatched::new(effect).with_transactions(transaction.into_iter().collect()))
}

fn blank(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

/// `vim.join`: `max(count, 2)` rows into one from each caret's (`:help J`).
/// Each joined row loses its indentation and gains one space, unless it's
/// empty, starts with `)`, joins an empty line, or follows a row that ends
/// in white space (as nvim reads it: the row before it, without its own
/// indentation, so a row of blanks doesn't count). The
/// caret goes to the last join. On the last row nothing is joined, though a
/// count of three or more moves the caret to the row's start, as in nvim;
/// `.` then repeats the count only as far as there were rows.
pub(crate) fn join(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let text = target.model.text().clone();
    let rows = args.count.unwrap_or(2).max(2);
    let last_row = text.row_count() - 1;
    let mut joined_to = None;
    let acts = target
        .model
        .selections()
        .iter()
        .map(|selection| {
            let first = text.offset_to_point(selection.head()).row;
            // Rows an earlier caret already joined.
            if joined_to.is_some_and(|to| first <= to) {
                return Acts::default();
            }
            // nvim joins nothing on the last row, but with a count of three
            // or more still moves to its start.
            if first == last_row {
                return Acts {
                    edits: Vec::new(),
                    land: (rows > 2).then(|| Land::At(text.row_start(first))),
                };
            }
            let last = first.saturating_add(rows - 1).min(last_row);
            joined_to = Some(last);
            let line = text.row_to_string(first);
            let mut empty = line.is_empty();
            // Whether the row joined last ends in white space, once its
            // indentation went.
            let mut after_blank = line.ends_with(blank);
            let mut acts = Acts::default();
            for row in first + 1..=last {
                let next = text.row_to_string(row);
                let rest = next.trim_start_matches(blank);
                let indent = next.len() - rest.len();
                let space = !rest.is_empty() && !empty && !after_blank && !rest.starts_with(')');
                let range = row_end(&text, row - 1)..text.row_start(row) + indent;
                acts.land = Some(Land::Edit(acts.edits.len(), 0));
                acts.edits
                    .push((range, if space { " " } else { "" }.to_string()));
                empty &= rest.is_empty();
                after_blank = rest.ends_with(blank);
            }
            acts
        })
        .collect();
    // `.` repeats the count as far as the primary caret's rows went.
    let primary = text
        .offset_to_point(target.model.primary_selection().head())
        .row;
    let count = args.count.map(|count| count.min(last_row - primary + 1));
    apply(target, actor, acts, Change::Join(CountArgs { count }))
}

/// `vim.replace_char`: the `count` graphemes from each caret become `char`
/// (a tab, spaces to as many tab stops), leaving the caret on the last;
/// nothing happens if fewer remain on the row. A line break isn't a
/// replacement here.
pub(crate) fn replace_char(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: ReplaceCharArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let count = args.count.unwrap_or(1).max(1) as usize;
    let acts = target
        .model
        .selections()
        .iter()
        .map(|selection| {
            let cursor = Cursor::at(&text, selection.head());
            let line = &cursor.line;
            if matches!(args.char, '\n' | '\r') || line.len() < cursor.ix.saturating_add(count) {
                return Acts::default();
            }
            let end = line.starts.get(cursor.ix + count).copied();
            let range = cursor.offset()..line.start + end.unwrap_or(line.text.len());
            let replacement = match args.char {
                // Each tab is spaces to the next tab stop, as the editor
                // types it (nvim's 'expandtab').
                '\t' => {
                    let from = line.start_column(cursor.ix, tab);
                    let to = (0..count).fold(from, |column, _| column + tab - column % tab);
                    " ".repeat(to - from)
                }
                ch => ch.to_string().repeat(count),
            };
            let last = replacement.chars().next_back().map_or(0, char::len_utf8);
            let land = replacement.len() - last;
            Acts {
                edits: vec![(range, replacement)],
                land: Some(Land::Edit(0, land)),
            }
        })
        .collect();
    apply(target, actor, acts, Change::ReplaceChar(args))
}

/// nvim's `~` for each code point it changes, hex `from:to` pairs written
/// by the ignored `record_case_table_with_neovim` test
/// (`tests/support/case.lua`).
const CASES: &str = include_str!("case.txt");

/// `ch` with its case toggled as nvim's `~` does it.
pub(crate) fn toggled(ch: char) -> char {
    static PAIRS: OnceLock<Vec<(u32, u32)>> = OnceLock::new();
    let pairs = PAIRS.get_or_init(|| {
        let hex = |field: &str| u32::from_str_radix(field, 16).expect("case.txt is hex");
        CASES
            .split_whitespace()
            .map(|pair| {
                let (from, to) = pair.split_once(':').expect("case.txt pairs are from:to");
                (hex(from), hex(to))
            })
            .collect()
    });
    match pairs.binary_search_by_key(&u32::from(ch), |&(from, _)| from) {
        Ok(ix) => char::from_u32(pairs[ix].1).expect("case.txt holds scalar values"),
        Err(_) => ch,
    }
}

/// `vim.toggle_case`: toggles the case of the `count` graphemes from each
/// caret (each one's first character), up to the row's end, and moves
/// past them, stopping on the last grapheme.
pub(crate) fn toggle_case(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let text = target.model.text().clone();
    let count = args.count.unwrap_or(1).max(1) as usize;
    let acts = target
        .model
        .selections()
        .iter()
        .map(|selection| {
            let cursor = Cursor::at(&text, selection.head());
            let line = &cursor.line;
            if line.len() == 0 {
                return Acts::default();
            }
            let end = cursor.ix.saturating_add(count).min(line.len());
            let range = cursor.offset()
                ..line.start + line.starts.get(end).copied().unwrap_or(line.text.len());
            let toggled: Vec<String> = (cursor.ix..end)
                .map(|ix| {
                    let mut chars = line.grapheme(ix).chars();
                    let first = chars.next().map(toggled);
                    first.into_iter().chain(chars).collect()
                })
                .collect();
            // Past the toggled graphemes, or on the last if they end the row.
            let land = match end < line.len() {
                true => toggled.iter().map(String::len).sum(),
                false => toggled[..toggled.len() - 1].iter().map(String::len).sum(),
            };
            let toggled = toggled.concat();
            if text.slice_to_string(range.clone()) == toggled {
                return Acts {
                    edits: Vec::new(),
                    land: Some(Land::At(range.start + land)),
                };
            }
            Acts {
                edits: vec![(range, toggled)],
                land: Some(Land::Edit(0, land)),
            }
        })
        .collect();
    apply(target, actor, acts, Change::ToggleCase(args))
}
