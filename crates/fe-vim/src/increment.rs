//! Increment: `<C-a>` and `<C-x>` (a feature module; see [`crate::feature`]).
//!
//! In normal mode, with a count and no operator, `<C-a>` and `<C-x>` commit
//! `vim.increment {"delta": 1 | -1, "count"}`: the first number at or after
//! each caret on its row changes by `count`, as nvim's does with its default
//! `nrformats=bin,hex`: decimal with a leading `-`, hex `0x` (the case of the
//! last letter of the old number kept) and binary `0b`, octal off. A number
//! that starts with a zero keeps its width, and the caret ends on its last
//! digit. Decimals saturate nowhere: they cross zero and wrap at 64 bits as
//! nvim's do; hex and binary wrap. A row with no number changes nothing and
//! leaves no undo step; `.` repeats it.
//!
//! In a visual mode the same keys change the first number on each selected
//! row (only inside the selection: a charwise one starts and ends mid-row,
//! a blockwise one is its columns), and `g<C-a>` and `g<C-x>`
//! (`"progressive": true`) add `count`, twice `count`, and so on, to the
//! rows that had one. The mode then ends with the caret at the selection's
//! start. `.` repeats it over a selection of the same size from the caret
//! (`"replay"`).

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::commands::origin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Change, Mode, Pending, Step, VimCommands, VimKey, VimTarget, VisualKind,
    commands::follow,
    feature::Feature,
    history, insert,
    motion::{Cursor, Line},
    operator,
    reducer::invocation,
    visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    z: None,
    register,
};

/// `vim.increment`'s args: `<C-a>` (`delta` 1) and `<C-x>` (`delta` -1), and
/// in a visual mode `g<C-a>` and `g<C-x>` (`progressive`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IncrementArgs {
    pub delta: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Each row that changed adds `count` more than the one before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub progressive: bool,
    /// Set on the change `.` repeats from a visual mode: the size of the
    /// selection it changed, laid over the caret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<Extent>,
}

/// The size of a visual selection.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Extent {
    pub kind: VisualKind,
    pub rows: u32,
    /// Graphemes in the last row (the only row, if `rows` is 1) of a
    /// charwise selection, display columns across a block.
    pub cols: usize,
}

fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let delta = match key {
        VimKey::Ctrl('a') => 1,
        VimKey::Ctrl('x') => -1,
        _ => return None,
    };
    command(pending, delta, false)
}

/// After `g`, in a visual mode.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let delta = match key {
        VimKey::Ctrl('a') => 1,
        VimKey::Ctrl('x') => -1,
        _ => return None,
    };
    if !pending.is_visual() {
        return None;
    }
    command(pending, delta, true)
}

fn command(pending: &Pending, delta: i32, progressive: bool) -> Option<Step> {
    if pending.has_operator() {
        return None;
    }
    Some(invocation(
        "vim.increment",
        IncrementArgs {
            delta,
            count: pending.count(),
            progressive,
            replay: None,
        },
    ))
}

fn register(registry: &mut Registry<VimCommands>) {
    let context = ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.increment",
            "Vim Increment",
            |target: &mut VimTarget<'_>, actor: &Actor, args: IncrementArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                increment(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Adds `count` to (or with a negative `delta` takes it from) the first number at or after each caret on its row, or in a visual mode on each selected row, as nvim's CTRL-A and CTRL-X do.",
        )
        .context(context)
        .mutates_buffer(true);
}

fn digit(b: Option<u8>) -> bool {
    b.is_some_and(|b| b.is_ascii_digit())
}

fn bin_digit(b: Option<u8>) -> bool {
    matches!(b, Some(b'0' | b'1'))
}

fn hex_digit(b: Option<u8>) -> bool {
    b.is_some_and(|b| b.is_ascii_hexdigit())
}

/// One number's new text.
struct Rewrite {
    range: Range<usize>,
    text: String,
}

/// `line`'s first number in `from..to` (in a visual mode; else at or after
/// `from`, the caret), changed by `amount` (taken from it if `subtract`): nvim's
/// `do_addsub`. A number's text beyond `to` isn't read.
fn rewrite(
    line: &str,
    from: usize,
    to: usize,
    visual: bool,
    subtract: bool,
    amount: u64,
) -> Option<Rewrite> {
    let b = &line.as_bytes()[..to];
    let at = |ix: usize| b.get(ix).copied();
    let mut col = from;
    if visual {
        while col < to && !digit(at(col)) {
            col += 1;
        }
    } else {
        while col > 0 && bin_digit(at(col)) {
            col -= 1;
        }
        while col > 0 && hex_digit(at(col)) {
            col -= 1;
        }
        let hex_at = |col: usize| {
            col > 0
                && matches!(at(col), Some(b'x' | b'X'))
                && at(col - 1) == Some(b'0')
                && hex_digit(at(col + 1))
        };
        if !hex_at(col) {
            col = from;
            while col > 0 && digit(at(col)) {
                col -= 1;
            }
        }
        if hex_at(col)
            || (col > 0
                && matches!(at(col), Some(b'b' | b'B'))
                && at(col - 1) == Some(b'0')
                && bin_digit(at(col + 1)))
        {
            col -= 1;
        } else {
            col = from;
            while col < to && !digit(at(col)) {
                col += 1;
            }
            while col > 0 && digit(at(col - 1)) {
                col -= 1;
            }
        }
    }
    if !digit(at(col)) {
        return None;
    }
    let first = b[col];
    // The prefix letter of a hex or binary number, and where its digits start.
    let (pre, digits_at) = match (at(col + 1), at(col + 2)) {
        (Some(x @ (b'x' | b'X')), d) if first == b'0' && hex_digit(d) => (Some(x), col + 2),
        (Some(x @ (b'b' | b'B')), d) if first == b'0' && bin_digit(d) => (Some(x), col + 2),
        _ => (None, col),
    };
    let radix = match pre {
        Some(b'x' | b'X') => 16,
        Some(_) => 2,
        None => 10,
    };
    let end = (digits_at..to)
        .find(|&ix| !(b[ix] as char).is_digit(radix))
        .unwrap_or(to);
    let mut n = 0u64;
    let mut overflow = false;
    for &d in &b[digits_at..end] {
        let d = u64::from((d as char).to_digit(16).expect("a digit"));
        // Past what 64 bits hold it is the maximum, which nvim doesn't change.
        match n
            .checked_mul(u64::from(radix))
            .and_then(|n| n.checked_add(d))
        {
            Some(next) if !overflow => n = next,
            _ => {
                overflow = true;
                n = u64::MAX;
            }
        }
    }
    let upper = b[col..end].iter().rev().find(|b| b.is_ascii_alphabetic());
    let upper = upper.is_some_and(u8::is_ascii_uppercase);
    // Whether a `-` before it is its sign.
    let mut negative =
        pre.is_none() && col > if visual { from } else { 0 } && at(col - 1) == Some(b'-');
    let was_negative = negative;
    let subtract = subtract ^ negative;
    let old = n;
    // Past 64 bits it stays the maximum, as nvim leaves it.
    if !overflow {
        n = if subtract {
            n.wrapping_sub(amount)
        } else {
            n.wrapping_add(amount)
        };
        if pre.is_none() {
            if subtract && n > old {
                n = 1 + (n ^ u64::MAX);
                negative = !negative;
            } else if !subtract && n < old {
                n ^= u64::MAX;
                negative = !negative;
            }
            if n == 0 {
                negative = false;
            }
        }
    }
    // The old text it replaces: its sign too, unless a visual mode's `-` stays.
    let mut start = col;
    let sign_in_range = was_negative && (!visual || !negative);
    if sign_in_range {
        start -= 1;
    }
    let digits = match (radix, upper) {
        (2, _) => format!("{n:b}"),
        (16, true) => format!("{n:X}"),
        (16, false) => format!("{n:x}"),
        _ => n.to_string(),
    };
    let mut text = String::new();
    let sign = negative && (!visual || !was_negative);
    if sign {
        text.push('-');
    }
    if let Some(letter) = pre {
        text.push('0');
        text.push(letter as char);
    }
    // The old width, if it began with a zero.
    if first == b'0' {
        let width = end - start;
        let zeros = width.saturating_sub(text.len() + digits.len());
        text.push_str(&"0".repeat(zeros));
    }
    text.push_str(&digits);
    Some(Rewrite {
        range: start..end,
        text,
    })
}

/// A row's bytes `from..to` to search.
struct Window {
    row: u32,
    from: usize,
    to: usize,
}

/// The windows a visual selection or its `extent` laid over `at` covers.
fn windows(
    target: &VimTarget<'_>,
    kind: VisualKind,
    ends: (usize, usize),
    extent: Option<Extent>,
) -> (Vec<Window>, Extent) {
    let text = target.model.text();
    let tab = target.model.tab_size().max(1);
    let last_row = text.row_count() - 1;
    let (lo, hi) = (ends.0.min(ends.1), ends.0.max(ends.1));
    let (first, last) = match extent {
        Some(extent) => {
            let first = text.offset_to_point(lo).row;
            (first, (first + extent.rows - 1).min(last_row))
        }
        None => (text.offset_to_point(lo).row, text.offset_to_point(hi).row),
    };
    // The grapheme at a byte of `line`, or the row's end.
    let ix_at = |line: &Line, byte: usize| line.starts.partition_point(|&start| start < byte);
    let end_of =
        |line: &Line, ix: usize| line.starts.get(ix + 1).copied().unwrap_or(line.text.len());
    let (lo_line, hi_line) = (Cursor::at(text, lo).line, Cursor::at(text, hi).line);
    let (lo_ix, hi_ix) = (Cursor::at(text, lo).ix, Cursor::at(text, hi).ix);
    // A block's display columns.
    let columns = match (kind, extent) {
        (VisualKind::Block, Some(extent)) => {
            let left = lo_line.start_column(lo_ix, tab);
            (left, left + extent.cols.max(1) - 1)
        }
        _ => {
            let (a, b) = (
                lo_line.start_column(lo_ix, tab),
                hi_line.start_column(hi_ix, tab),
            );
            (a.min(b), a.max(b))
        }
    };
    let mut cols = 0;
    let windows = (first..=last)
        .map(|row| {
            let line = Line::new(text, row);
            let len = line.text.len();
            let (from, to) = match kind {
                VisualKind::Line => (0, len),
                VisualKind::Char => {
                    let from = if row == first { lo - line.start } else { 0 };
                    let to = match (extent, row == last) {
                        (Some(extent), true) => {
                            let ix = match first == last {
                                true => ix_at(&line, from) + extent.cols,
                                false => extent.cols,
                            };
                            line.starts.get(ix).copied().unwrap_or(len)
                        }
                        (None, true) => match line.len() {
                            0 => 0,
                            n => end_of(&line, hi_ix.min(n - 1)),
                        },
                        _ => len,
                    };
                    (from, to)
                }
                VisualKind::Block => {
                    let inside: Vec<usize> = (0..line.len())
                        .filter(|&ix| {
                            let column = line.start_column(ix, tab);
                            columns.0 <= column && column <= columns.1
                        })
                        .collect();
                    match (inside.first(), inside.last()) {
                        (Some(&a), Some(&z)) => (line.starts[a], end_of(&line, z)),
                        _ => (0, 0),
                    }
                }
            };
            if row == last {
                cols = match kind {
                    VisualKind::Block => columns.1 - columns.0 + 1,
                    _ => {
                        let (a, z) = (
                            ix_at(&line, if first == last { from } else { 0 }),
                            ix_at(&line, to),
                        );
                        z - a
                    }
                };
            }
            Window { row, from, to }
        })
        .collect();
    let rows = last - first + 1;
    (windows, Extent { kind, rows, cols })
}

pub(crate) fn increment(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: IncrementArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let before = target.model.selections();
    let mut carets_before = history::pairs(target.model);
    let live = match target.state.mode() {
        Mode::Visual(kind) => {
            let ends = (
                target.state.visual_anchor(target.model),
                target.state.visual_cursor(target.model),
            );
            ends.0.zip(ends.1).map(|ends| (kind, ends))
        }
        _ => None,
    };
    // Where each search starts: (row, caret byte or window).
    let text = target.model.text().clone();
    let (searches, extent, start) = match (live, args.replay) {
        (Some((kind, ends)), _) => {
            let (windows, extent) = windows(target, kind, ends, None);
            let start = ends.0.min(ends.1);
            (windows, Some(extent), start)
        }
        (None, Some(extent)) => {
            let head = target.model.primary_selection().head();
            let (windows, _) = windows(target, extent.kind, (head, head), Some(extent));
            (windows, Some(extent), head)
        }
        (None, None) => {
            let mut rows = Vec::new();
            for selection in &before {
                let cursor = Cursor::at(&text, selection.head());
                if rows.last().is_none_or(|w: &Window| w.row != cursor.row) {
                    let from = selection.head() - cursor.line.start;
                    rows.push(Window {
                        row: cursor.row,
                        from,
                        to: cursor.line.text.len(),
                    });
                }
            }
            (rows, None, 0)
        }
    };
    let visual = extent.is_some();
    if live.is_some() {
        // Ends the mode first, so its marks keep the columns they had.
        carets_before = vec![history::pair(&text, start)];
        visual::exit(target)?;
    }
    // nvim takes counts only to 999999999.
    let count = u64::from(args.count.unwrap_or(1).clamp(1, 999_999_999));
    let mut amount = count;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut heads = Vec::new();
    for window in &searches {
        let line = text.row_to_string(window.row);
        let row_start = text.row_start(window.row);
        let subtract = args.delta < 0;
        let found = rewrite(&line, window.from, window.to, visual, subtract, amount);
        if let Some(Rewrite { range, text: new }) = found {
            heads.push(window.row);
            edits.push((row_start + range.start..row_start + range.end, new));
            if args.progressive {
                amount = amount.saturating_add(count);
            }
        }
    }
    // Where each edit's caret lands once the edits are in, and the shift
    // each makes to what follows it.
    let mut shift = 0isize;
    let mut landed = Vec::new();
    let mut shifts = Vec::new();
    for (ix, (range, new)) in edits.iter().enumerate() {
        let start = range.start.checked_add_signed(shift).expect("sorted edits");
        landed.push((heads[ix], start + new.len() - 1));
        shift += new.len() as isize - range.len() as isize;
        shifts.push((range.end, shift));
    }
    let after = |offset: usize| {
        let shift = shifts
            .iter()
            .take_while(|(end, _)| *end <= offset)
            .last()
            .map_or(0, |&(_, shift)| shift);
        offset
            .checked_add_signed(shift)
            .expect("an offset past its edits")
    };
    let (transaction, snapshot) = match edits.is_empty() {
        true => (None, target.model.snapshot().clone()),
        false => operator::apply(target.model, edits, origin(actor)),
    };
    if let Some(id) = transaction {
        target.state.carets.record(id, carets_before);
        let args = IncrementArgs {
            replay: extent,
            ..args
        };
        target.state.set_last_change(Change::Increment(args).into());
        target.state.goals = None;
    }
    let carets: Vec<usize> = match live {
        Some(_) => vec![after(start)],
        None => before
            .iter()
            .map(|selection| {
                let head = selection.head();
                let row = text.offset_to_point(head).row;
                match landed.iter().find(|(edited, _)| *edited == row) {
                    Some(&(_, caret)) => caret,
                    None => after(head),
                }
            })
            .collect(),
    };
    // nvim fails a change that finds no number.
    target.state.failed |= transaction.is_none();
    operator::land(target.model, &snapshot, carets);
    let effect = match transaction.is_some() || live.is_some() {
        true => Effect::Changed,
        false => Effect::Nothing,
    };
    Ok(Dispatched::new(effect).with_transactions(transaction.into_iter().collect()))
}
