//! `[<Space>` and `]<Space>` (nvim 0.11's defaults, `:help [<Space>`), and
//! `[e` and `]e` (vim-unimpaired's), a feature module; see
//! [`crate::feature`].
//!
//! `]<Space>` and `[<Space>` commit `vim.blank_lines` ([`BlankLinesArgs`]):
//! `count` empty rows go in below or above the caret's row, each caret's
//! once, in one undo step. The caret stays on its text at its column (a row
//! put in above moves it down), and marks below, and those on the row for
//! rows put in above, move down with their rows, as in nvim, where the
//! default puts the rows in with `nvim_buf_set_lines` from a `g@l`: so
//! `'[` and `']` are where the caret was, and `'.` and the changelist's
//! entry at the first row put in. `.` repeats it, a count given to `.`
//! replacing its own. More rows than a put may insert ([`MAX_PUT`] bytes)
//! fail with nothing changed. nvim maps them in
//! normal mode only: in a visual mode `[` then `<Space>` cancels, keeping
//! the selection, as there.
//!
//! `]e` and `[e` commit `vim.move_rows` ([`MoveRowsArgs`]): the caret's row
//! goes `count` rows down or up, the rows it passes each moving one row the
//! other way, as unimpaired's `:move +{count}` and `:move --{count}` do.
//! With several carets their rows move together, each by the same rows;
//! in a visual mode the selection's rows do, the selection kept on them
//! (as mini.move's, where unimpaired's leaves the visual mode). Near the
//! buffer's edge they move as far as they can; with no row left that way
//! it fails with nothing changed (unimpaired's `:move` refuses a count past
//! the edge, E16). One undo step, not a jump. Each caret goes with its row
//! at its column, and the rows' marks, jumplist and changelist entries go
//! with their text, as `:move`'s do in nvim; `'[` and `']` go to the start
//! of the first and last row moved, and `'.` and the changelist's entry to
//! the first row changed. As nvim's undo header saves `a` to `z` and the
//! last visual extent with a change, `u` puts back those from before the
//! first move of the change and `<C-r>` those from before the `u`
//! ([`Moves`], which every change records); the other marks and entries
//! stay, as there. Rows of the
//! same text moving past each other change nothing, but are still an undo
//! step. `.` repeats it from the caret,
//! over as many rows as the selection had after a visual one.
//!
//! After an operator neither is a motion or an object: `d]e` and `d[<Space>`
//! cancel.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::commands::origin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use kiln_l10n::tr;

use crate::{
    Change, Mode, Pending, Step, VimCommands, VimKey, VimTarget,
    feature::Feature,
    g_operators, history, insert, marks,
    operator::{self, MAX_PUT, row_end},
    reducer::invocation,
    visual,
};

pub(crate) use crate::history::Moves;

pub(crate) const FEATURE: Feature = Feature {
    bracket: Some(bracket),
    register,
    ..Feature::NONE
};

/// `vim.blank_lines`'s args, and what `.` repeats after `[<Space>` or
/// `]<Space>`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BlankLinesArgs {
    /// `[<Space>` if true, else `]<Space>`.
    pub above: bool,
    /// How many rows to put in: 1 if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.move_rows`'s args, and what `.` repeats after `[e` or `]e`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MoveRowsArgs {
    /// `[e` if true, else `]e`.
    pub up: bool,
    /// How many rows to move them: 1 if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// In normal mode, this many rows from the caret's move, rather than
    /// each caret's row: what `.` repeats after a visual one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u32>,
}

/// Where rows moving went: from `top`, the row each came from (`order`) and
/// where each went (`to`).
struct Places {
    top: u32,
    order: Vec<u32>,
    to: Vec<u32>,
}

impl Places {
    /// `rows` (sorted, each once) moving `by` rows down, or `up`.
    fn of(rows: &[u32], up: bool, by: u32) -> Self {
        let order = reordered(rows, up, by);
        let top = match up {
            true => rows[0] - by,
            false => rows[0],
        };
        let mut to = vec![0; order.len()];
        for (at, &from) in (top..).zip(&order) {
            to[(from - top) as usize] = at;
        }
        Self { top, order, to }
    }

    /// Where row `row` went, if the move changed rows there.
    fn went(&self, row: u32) -> Option<u32> {
        self.to.get(row.checked_sub(self.top)? as usize).copied()
    }
}

/// `<Space>` and `e` after `[` or `]`, with no operator pending; `<Space>`
/// only in normal mode.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    if pending.has_operator() {
        return None;
    }
    match key {
        VimKey::Char(' ') if !pending.is_visual() => Some(invocation(
            "vim.blank_lines",
            BlankLinesArgs {
                above: !forward,
                count: pending.count(),
            },
        )),
        VimKey::Char('e') => Some(invocation(
            "vim.move_rows",
            MoveRowsArgs {
                up: !forward,
                count: pending.count(),
                rows: None,
            },
        )),
        _ => None,
    }
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == normal"))
        .expect("vim's context predicate parses");
    let normal_or_visual = ContextPredicate::parse(kiln_l10n::verbatim!(
        "editor && (vim.mode == normal || vim.visual)"
    ))
    .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.blank_lines",
            &crate::l10n::command_title("vim.blank_lines"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: BlankLinesArgs| {
                crate::commands::follow(target);
                insert::adopt_closing(target.state);
                Ok::<_, CommandError>(blank_lines(target, actor, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.blank_lines"))
        .context(normal)
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.move_rows",
            &crate::l10n::command_title("vim.move_rows"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: MoveRowsArgs| {
                crate::commands::follow(target);
                insert::adopt_closing(target.state);
                Ok::<_, CommandError>(move_rows(target, actor, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.move_rows"))
        .context(normal_or_visual)
        .mutates_buffer(true);
}

/// `vim.blank_lines`.
pub(crate) fn blank_lines(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: BlankLinesArgs,
) -> Dispatched<TransactionId> {
    let count = args.count.unwrap_or(1).max(1) as usize;
    let snapshot = target.model.snapshot().clone();
    let text = snapshot.text();
    let carets = history::pairs(target.model);
    let mut rows: Vec<u32> = carets.iter().map(|&(row, _)| row).collect();
    rows.sort_unstable();
    rows.dedup();
    if count
        .checked_mul(rows.len())
        .is_none_or(|size| size > MAX_PUT)
    {
        return target.state.fail(tr!(
            "vim-blank-rows-too-large",
            count = count,
            limit = MAX_PUT
        ));
    }
    let breaks = "\n".repeat(count);
    let edits: Vec<_> = rows
        .iter()
        .map(|&row| {
            let at = match args.above {
                true => text.row_start(row),
                false => row_end(text, row),
            };
            (at..at, breaks.clone())
        })
        .collect();
    for &row in &rows {
        match args.above {
            // Marks on an empty row move down with it, as on any other.
            true => target.state.marks.opening(&snapshot, row),
            // And `'<` and `'>` on it stay, as the other marks do.
            false => visual::opening_below(target.state, &snapshot, row),
        }
    }
    let (transaction, after) = operator::apply(target.model, edits, origin(actor));
    if !args.above {
        visual::reanchored(target.state, &after);
    }
    // Each caret stays on its text, below the rows put in above it.
    let heads = carets.iter().map(|&(row, column)| {
        let above = match args.above {
            true => rows.iter().filter(|&&at| at <= row).count(),
            false => rows.iter().filter(|&&at| at < row).count(),
        };
        let row = row + (above * count) as u32;
        after.text().row_start(row) + column
    });
    operator::land(target.model, &after, heads.collect::<Vec<_>>());
    let (row, column) = carets[0];
    if let Some(id) = transaction {
        target.state.carets.record(id, carets);
    }
    let first = row + u32::from(!args.above);
    marks::opened_blank(
        target.state,
        target.model,
        transaction,
        (row, column),
        first,
    );
    target
        .state
        .set_last_change(Change::BlankLines(args).into());
    Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
}

/// `vim.move_rows`.
pub(crate) fn move_rows(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: MoveRowsArgs,
) -> Dispatched<TransactionId> {
    target.model.sync_display();
    let snapshot = target.model.snapshot().clone();
    let text = snapshot.text();
    let last_row = text.row_count() - 1;
    let carets = history::pairs(target.model);
    let pair = |offset| history::pair(text, offset);
    let visual = match target.state.mode {
        Mode::Visual(_) => target
            .state
            .visual_anchor(target.model)
            .zip(target.state.visual_cursor(target.model))
            .map(|(anchor, cursor)| (pair(anchor), pair(cursor))),
        _ => None,
    };
    let rows: Vec<u32> = match (visual, args.rows) {
        (Some(((anchor, _), (cursor, _))), _) => {
            (anchor.min(cursor)..=anchor.max(cursor)).collect()
        }
        (None, Some(rows)) => {
            let row = carets[0].0;
            (row..=row.saturating_add(rows.max(1) - 1).min(last_row)).collect()
        }
        (None, None) => {
            let mut rows: Vec<u32> = carets.iter().map(|&(row, _)| row).collect();
            rows.sort_unstable();
            rows.dedup();
            rows
        }
    };
    let (first, last) = (rows[0], rows[rows.len() - 1]);
    let room = match args.up {
        true => first,
        false => last_row - last,
    };
    let by = args.count.unwrap_or(1).max(1).min(room);
    if by == 0 {
        target.state.failed = true;
        return Dispatched::new(Effect::Nothing);
    }
    // Where each row goes: the moved ones `by` rows over, the rest of the
    // rows from the first changed filling in after each other.
    let places = Places::of(&rows, args.up, by);
    let (top, order) = (places.top, &places.order);
    let moved = |row: u32| places.went(row);
    target.state.marks.moving(&snapshot, &moved);
    visual::moving_rows(target.state, &snapshot, &moved);
    let edits: Vec<_> = (top..)
        .zip(order)
        .filter(|&(row, &from)| row != from)
        .map(|(row, &from)| {
            let range = text.row_start(row)..row_end(text, row);
            (range, text.row_to_string(from))
        })
        .filter(|(range, line)| text.slice_to_string(range.clone()) != *line)
        .collect();
    // Rows of the same text moved past each other change nothing, but are
    // still an undo step.
    let (transaction, after, _) =
        g_operators::apply_noted(target.state, target.model, edits, actor);
    let at = |(row, column): (u32, usize)| {
        let row = moved(row).unwrap_or(row);
        after.text().row_start(row) + column
    };
    match visual {
        Some((anchor, cursor)) => {
            let (anchor, cursor) = (at(anchor), at(cursor));
            target.state.remap_visual(target.model, anchor, cursor);
        }
        None => {
            let heads: Vec<usize> = carets.iter().map(|&caret| at(caret)).collect();
            operator::land(target.model, &after, heads);
        }
    }
    if let Some(id) = transaction {
        // `u` puts the caret back where the vim cursor was.
        let before = match visual {
            Some((_, cursor)) => vec![cursor],
            None => carets,
        };
        target.state.carets.record(id, before);
    }
    let landed = |row| moved(row).unwrap_or(row);
    let ends = (landed(first), landed(last));
    marks::moved(target.state, target.model, transaction, ends, top);
    target.state.goals = None;
    let rows = visual.map(|_| last - first + 1).or(args.rows);
    target
        .state
        .set_last_change(Change::MoveRows(MoveRowsArgs { rows, ..args }).into());
    Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
}

/// The rows from the first that `rows` (sorted, each once) moving `by` rows
/// down, or `up`, passes over, in their new order: `rows` each `by` rows
/// over, the rest filling in after each other.
fn reordered(rows: &[u32], up: bool, by: u32) -> Vec<u32> {
    let (first, last) = (rows[0], rows[rows.len() - 1]);
    let (top, bottom) = match up {
        true => (first - by, last),
        false => (first, last + by),
    };
    let mut order: Vec<Option<u32>> = vec![None; (bottom - top + 1) as usize];
    for &row in rows {
        let to = match up {
            true => row - by,
            false => row + by,
        };
        order[(to - top) as usize] = Some(row);
    }
    let mut rest = (top..=bottom).filter(|row| rows.binary_search(row).is_err());
    order
        .into_iter()
        .map(|slot| slot.or_else(|| rest.next()).expect("a row for every slot"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::reordered;

    #[test]
    fn reordered_moves_rows_over_the_rest() {
        // One row down one: it swaps with the next.
        assert_eq!(reordered(&[1], false, 1), [2, 1]);
        // Down two: the two it passes each go up one.
        assert_eq!(reordered(&[1], false, 2), [2, 3, 1]);
        assert_eq!(reordered(&[3], true, 2), [3, 1, 2]);
        // A run of rows moves whole.
        assert_eq!(reordered(&[1, 2], false, 1), [3, 1, 2]);
        assert_eq!(reordered(&[2, 3], true, 1), [2, 3, 1]);
        // Rows apart each swap with the row past them.
        assert_eq!(reordered(&[1, 3], false, 1), [2, 1, 4, 3]);
        assert_eq!(reordered(&[0, 2], true, 0), [0, 1, 2]);
    }
}
