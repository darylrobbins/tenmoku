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
//! leaves no undo step; `.` still repeats the command (with its count, from
//! the new caret), and only normal mode fails, so a macro stops there.
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

use kiln_l10n::tr;

use crate::{
    Change, Mode, Pending, Step, VimCommands, VimKey, VimTarget, VisualKind, commands::follow,
    feature::Feature, history, insert, motion::Cursor, operator, reducer::invocation, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    z: None,
    register,
    ..Feature::NONE
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

/// The size of a visual selection, as `.` lays it over the caret.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Extent {
    pub kind: VisualKind,
    /// At least 1.
    pub rows: u32,
    /// Display columns, at least 1: those of the last row (the only row, if
    /// `rows` is 1) of a charwise selection, counted from the start of the
    /// selection (from the row's start across several rows), and a block's
    /// width. `None` after `$`, to each row's end, and for a linewise one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<usize>,
}

/// Wider than any row: what `cols` is cut to, so sums with a column don't
/// overflow.
const MAX_COLS: usize = 1 << 32;

impl Extent {
    fn of(extent: visual::Extent) -> Self {
        let (kind, rows, cols) = match extent {
            visual::Extent::Lines(rows) => (VisualKind::Line, rows, None),
            visual::Extent::Chars { rows, last } => (VisualKind::Char, rows, last),
            visual::Extent::Block { rows, columns } => (VisualKind::Block, rows, columns),
        };
        Self { kind, rows, cols }
    }

    /// The extent it names, or why it names none.
    fn visual(self) -> Result<visual::Extent, CommandError> {
        let Self { kind, rows, cols } = self;
        if rows == 0 || cols == Some(0) {
            return Err(CommandError::new(tr!("vim-increment-empty-extent")));
        }
        let cols = cols.map(|cols| cols.min(MAX_COLS));
        Ok(match kind {
            VisualKind::Line => visual::Extent::Lines(rows),
            VisualKind::Char => visual::Extent::Chars { rows, last: cols },
            VisualKind::Block => visual::Extent::Block {
                rows,
                columns: cols,
            },
        })
    }
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
    let context = ContextPredicate::parse(kiln_l10n::verbatim!(
        "editor && (vim.mode == normal || vim.visual)"
    ))
    .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.increment",
            &crate::l10n::command_title("vim.increment"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: IncrementArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                increment(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.increment"))
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
    // The old width of its digits (not its sign), if it began with a zero.
    if first == b'0' {
        let zeros = (end - col).saturating_sub(text.len() - usize::from(sign) + digits.len());
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

pub(crate) fn increment(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: IncrementArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    // Checked before anything changes.
    let replay = args.replay.map(Extent::visual).transpose()?;
    target.model.sync_display();
    let before = target.model.selections();
    let mut carets_before = history::pairs(target.model);
    let live = match target.state.mode() {
        Mode::Visual(_) => visual::live_area(target.model, target.state),
        _ => None,
    };
    let ended = live.is_some();
    let area = live
        .or_else(|| replay.map(|extent| visual::replay_area(target.model, target.state, &extent)));
    let text = target.model.text().clone();
    // Where each search runs: a row's bytes in the area, or from each caret.
    let windows: Vec<Window> = match &area {
        Some(area) => area
            .rows
            .iter()
            .map(|(row, range)| {
                let row_start = text.row_start(*row);
                Window {
                    row: *row,
                    from: range.start - row_start,
                    to: range.end - row_start,
                }
            })
            .collect(),
        None => before
            .iter()
            .map(|selection| {
                let cursor = Cursor::whole(&text, selection.head());
                Window {
                    row: cursor.row,
                    from: selection.head() - cursor.line.start,
                    to: cursor.line.text.len(),
                }
            })
            .collect(),
    };
    let visual = area.is_some();
    if let Some(area) = area.as_ref().filter(|_| ended) {
        // Ends the mode first, so its marks keep the columns they had.
        carets_before = vec![history::pair(&text, area.start)];
        visual::exit(target)?;
    }
    // nvim takes counts only to 999999999.
    let count = u64::from(args.count.unwrap_or(1).clamp(1, 999_999_999));
    let mut amount = count;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    // Which edit each window's number is, if it has one.
    let mut edit_of: Vec<Option<usize>> = Vec::new();
    for window in &windows {
        let line = text.row_to_string(window.row);
        let row_start = text.row_start(window.row);
        let subtract = args.delta < 0;
        let found = rewrite(&line, window.from, window.to, visual, subtract, amount);
        let Some(Rewrite { range, text: new }) = found else {
            edit_of.push(None);
            continue;
        };
        let range = row_start + range.start..row_start + range.end;
        // A number that several carets found changes once.
        match edits.last() {
            Some((last, _)) if last.start < range.end && range.start < last.end => {
                edit_of.push(Some(edits.len() - 1));
            }
            _ => {
                edit_of.push(Some(edits.len()));
                edits.push((range, new));
                if args.progressive {
                    amount = amount.saturating_add(count);
                }
            }
        }
    }
    // Where each edit's caret lands once the edits are in, and the shift
    // each makes to what follows it.
    let mut shift = 0isize;
    let mut landed = Vec::new();
    let mut shifts = Vec::new();
    for (range, new) in &edits {
        let start = range.start.checked_add_signed(shift).expect("sorted edits");
        landed.push(start + new.len() - 1);
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
        target.state.goals = None;
    }
    // A change that found no number is still the change `.` repeats.
    let replay = match (&area, ended) {
        (Some(area), true) => Some(Extent::of(area.extent)),
        _ => args.replay,
    };
    let repeat = IncrementArgs { replay, ..args };
    target
        .state
        .set_last_change(Change::Increment(repeat).into());
    // A selection leaves the caret at its start; a number's last digit
    // otherwise.
    let carets: Vec<usize> = match &area {
        Some(area) => vec![after(area.start)],
        None => before
            .iter()
            .zip(&edit_of)
            .map(|(selection, edit)| match edit {
                Some(ix) => landed[*ix],
                None => after(selection.head()),
            })
            .collect(),
    };
    // nvim fails a normal-mode change that finds no number; over a
    // selection it ends the mode and a macro goes on.
    target.state.failed |= transaction.is_none() && area.is_none();
    operator::land(target.model, &snapshot, carets);
    let effect = match transaction.is_some() || ended {
        true => Effect::Changed,
        false => Effect::Nothing,
    };
    Ok(Dispatched::new(effect).with_transactions(transaction.into_iter().collect()))
}
