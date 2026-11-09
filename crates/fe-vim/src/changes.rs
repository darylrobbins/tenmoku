//! `]c` and `[c`, and `do` and `dp` (`:help ]c`, `:help do`), a feature
//! module; see [`crate::feature`].
//!
//! `[` and `]` take a key after them (a [`Prefix::Bracket`] in the
//! reducer): this module answers `c`, and later `[`/`]` keys (`[(`, `]]`,
//! `[m`) join its prefix. Any other key cancels. With an operator pending
//! `c` cancels too, so `d]c` does (diff mode's is out of scope).
//!
//! In normal and the visual modes `]c` and `[c` commit `vim.change`
//! ([`ChangeArgs`]): the caret goes to the first row of the `count`th next
//! (`]c`) or previous (`[c`) change in [`EditorModel::row_changes`], a
//! deleted-rows change at the row it's reported at. Column 0, where nvim's
//! `]c` lands in diff mode (checked by hand with `:diffthis` on two windows,
//! from the end of a long row, onto an indented row). Fewer changes than
//! the count go as far as there are, as nvim does; none that way fails as a
//! failed motion does, with no wrap. A jump: it sets `''`. In a visual mode
//! it extends the selection.
//!
//! An editor with a git diff base gets gitsigns' `]c` and `[c`; in the
//! diff view each side's base is the other side, so they are diff mode's.
//!
//! `do` and `dp` (an operator `d` pending, then `o` or `p`; a count or
//! register is ignored) run what `:diffget` and `:diffput` run for the
//! cursor's row, so the host decides: outside a diff view it answers
//! `E99: Current buffer is not in diff mode`.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Mode, MoveArgs, Operator, Pending, Step, VimCommands, VimKey, VimTarget, args::Motion, ex,
    feature::Feature, marks, motion, reducer::invocation, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    bracket: Some(bracket),
    register,
    ..Feature::NONE
};

/// `vim.change`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChangeArgs {
    /// `]c` if true, else `[c`.
    pub forward: bool,
    /// How many changes to go: 1 if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `o` and `p` after `d`.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if pending.operator() != Some(Operator::Delete) || pending.has_prefix() || pending.is_visual() {
        return None;
    }
    match key {
        VimKey::Char('o') => Some(invocation("vim.diff_get", ())),
        VimKey::Char('p') => Some(invocation("vim.diff_put", ())),
        _ => None,
    }
}

/// `c` after `[` or `]`.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    match key {
        // Diff mode's `d]c` is out of scope: an operator cancels.
        VimKey::Char('c') if pending.operator().is_none() => Some(invocation(
            "vim.change",
            ChangeArgs {
                forward,
                count: pending.count(),
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
            "vim.change",
            &crate::l10n::command_title("vim.change"),
            |target: &mut VimTarget<'_>, _: &Actor, args: ChangeArgs| {
                Ok::<_, CommandError>(run(target, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.change"))
        .context(normal_or_visual)
        .mutates_buffer(false);
    for (id, line) in [("vim.diff_get", "diffget"), ("vim.diff_put", "diffput")] {
        registry
            .register(
                id,
                &crate::l10n::command_title(id),
                move |target: &mut VimTarget<'_>, actor: &Actor| ex::run(target, actor, line),
            )
            .expect("vim command IDs are valid and distinct")
            .description(&crate::l10n::command_description(id))
            .context(normal.clone())
            .mutates_buffer(false);
    }
}

/// `]c` or `[c`.
fn run(target: &mut VimTarget<'_>, args: ChangeArgs) -> Dispatched<TransactionId> {
    target.model.sync_display();
    let head = target
        .state
        .visual_cursor(target.model)
        .unwrap_or_else(|| target.model.primary_selection().head());
    let text = target.model.text().clone();
    let row = text.offset_to_point(head).row;
    let last = text.row_count().saturating_sub(1);
    let starts = target
        .model
        .row_changes()
        .iter()
        .map(|change| change.rows.start.min(last))
        .collect::<Vec<_>>();
    let mut count = args.count.unwrap_or(1).max(1) as usize;
    let found = if args.forward {
        let after: Vec<_> = starts.iter().filter(|&&start| start > row).collect();
        count = count.min(after.len());
        count.checked_sub(1).map(|ix| *after[ix])
    } else {
        let before: Vec<_> = starts.iter().filter(|&&start| start < row).collect();
        count = count.min(before.len());
        count
            .checked_sub(1)
            .map(|ix| *before[before.len() - 1 - ix])
    };
    let Some(row) = found else {
        target.state.failed = true;
        return Dispatched::new(Effect::Nothing);
    };
    let offset = text.row_start(row);
    marks::jump_here(target);
    let moved = match target.state.mode {
        Mode::Visual(_) => visual::jump_cursor(target.model, target.state, offset),
        _ => {
            // nvim's `]c` sets the column without touching `curswant`: the
            // goal `j` and `k` would have kept survives the jump.
            let version = target.model.snapshot().version();
            let before = target.model.selections();
            let goal = motion::valid_goals(target.state, version, &before)
                .and_then(|(_, goals)| goals.iter().find(|(at, _)| *at == head))
                .and_then(|(_, goal)| *goal)
                .unwrap_or_else(|| {
                    let cursor = motion::Cursor::at(&text, head);
                    cursor
                        .line
                        .goal_column(cursor.ix, target.model.tab_size().max(1))
                });
            let args = MoveArgs {
                motion: Motion::MarkAt {
                    offset,
                    exact: true,
                },
                extend: false,
                count: None,
            };
            let moved = motion::apply(target.model, target.state, args);
            if let Some((_, goals)) = target.state.goals.as_mut() {
                for (_, kept) in goals {
                    *kept = Some(goal);
                }
            }
            moved
        }
    };
    marks::settle(target);
    Dispatched::new(match moved {
        true => Effect::Changed,
        false => Effect::Nothing,
    })
}
