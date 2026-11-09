//! The changelist (`:help changelist`): `g;` and `g,` (a feature module;
//! see [`crate::feature`]).
//!
//! With no operator pending, `g;` commits `vim.change_older {"count"}` and
//! `g,` `vim.change_newer {"count"}` ([`ChangeArgs`]); with one they aren't
//! vim's. In a visual mode they fail with nothing changed, the mode going
//! on, as nvim's beep.
//!
//! The list is the marks' (`marks.rs`), one per [`VimState`], with `'.`:
//! each change of vim's (each of its edit batches that sets `'.`) makes
//! `'.` its newest entry, added if the change is the first of a new
//! undoable change and isn't on the newest entry's row within 79 columns of
//! it, as nvim's are. At most 100 are kept, the oldest going, and they
//! follow edits as jumplist entries do. `g;` goes `count` entries back
//! (towards older changes) and `g,` forward, to the entry's exact position
//! (clamped as a caret is), a count past either end going to that end;
//! from that end they fail (E662 and E663) and with no change made E664,
//! changing nothing. Neither is a jump.
//!
//! Known limits: each change a macro's run makes is an undoable change of
//! its own, where nvim's run is one, adding one entry; and `:s` leaves `'.`
//! (so the entry) at its first match, where nvim's is at column 0 of the
//! first row it changed.
//!
//! [`VimState`]: crate::VimState

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_text::Bias;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Mode, MoveArgs, Pending, Step, VimCommands, VimKey, VimTarget, args::Motion, feature::Feature,
    marks, motion, reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    g,
    register,
    ..Feature::NONE
};

/// `vim.change_older`'s and `vim.change_newer`'s args.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ChangeArgs {
    /// How many entries to go: 1 if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) count: Option<u32>,
}

fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let command = match key {
        VimKey::Char(';') => "vim.change_older",
        VimKey::Char(',') => "vim.change_newer",
        _ => return None,
    };
    (!pending.has_operator()).then(|| {
        invocation(
            command,
            ChangeArgs {
                count: pending.count(),
            },
        )
    })
}

fn register(registry: &mut Registry<VimCommands>) {
    let context = ContextPredicate::parse(kiln_l10n::verbatim!(
        "editor && (vim.mode == normal || vim.visual)"
    ))
    .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.change_older",
            &crate::l10n::command_title("vim.change_older"),
            |target: &mut VimTarget<'_>, _: &Actor, args: ChangeArgs| {
                Ok::<_, CommandError>(run(target, args, true))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.change_older"))
        .context(context.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.change_newer",
            &crate::l10n::command_title("vim.change_newer"),
            |target: &mut VimTarget<'_>, _: &Actor, args: ChangeArgs| {
                Ok::<_, CommandError>(run(target, args, false))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.change_newer"))
        .context(context)
        .mutates_buffer(false);
}

/// `g;` (`older`) or `g,`.
fn run(target: &mut VimTarget<'_>, args: ChangeArgs, older: bool) -> Dispatched<TransactionId> {
    if let Mode::Visual(_) = target.state.mode {
        // nvim beeps, with no message.
        target.state.failed = true;
        return Dispatched::new(Effect::Nothing);
    }
    target.model.sync_display();
    target.state.marks.watch(target.model);
    marks::catch_up(target.state);
    let snapshot = target.model.snapshot().clone();
    let count = args.count.unwrap_or(1);
    let (row, column) = match target.state.marks.go_change(&snapshot, older, count) {
        Ok(Some(found)) => found,
        Ok(None) => {
            return target
                .state
                .fail(crate::messages::mark_has_invalid_line_number());
        }
        Err(message) => return target.state.fail(message),
    };
    let text = snapshot.text();
    let offset = text.row_start(row) + column.min(text.row_len(row) as usize);
    let args = MoveArgs {
        motion: Motion::MarkAt {
            offset: text.clip_offset(offset, Bias::Left),
            exact: true,
        },
        extend: false,
        count: None,
    };
    Dispatched::new(match motion::apply(target.model, target.state, args) {
        true => Effect::Changed,
        false => Effect::Nothing,
    })
}
