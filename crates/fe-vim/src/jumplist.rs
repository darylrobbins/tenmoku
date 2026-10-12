//! The jumplist (`:help jumplist`): `<C-o>`, `<C-i>` and `<Tab>` (a
//! feature module; see [`crate::feature`]).
//!
//! In normal mode, with nothing but a count pending, `<C-o>` commits
//! `vim.jump_older {"count"}` and `<C-i>` or `<Tab>` `vim.jump_newer
//! {"count"}` ([`JumpArgs`]). In a visual or operator-pending mode they
//! aren't vim's.
//!
//! The list is the marks' (`marks.rs`), one per [`VimState`], as nvim's
//! (with its default 'jumpoptions') is:
//!
//! - Each jump that sets the jump mark `''` (`gg G % / ? n N * #`, a mark's
//!   motion, `''` itself, `{count}zt` and the like, `:d`, `:s`, `:g` and
//!   `:>`, with an operator too) adds where it left from, and so does `m'`.
//!   At most 100 entries are kept, the oldest going.
//! - Entries follow edits as marks do, but a deleted row doesn't delete one:
//!   it goes to the row that took its place, at the same column, or past the
//!   last row where none did, where going to it fails with E19.
//! - When the list is used, an entry goes if a later one is on its row, so
//!   `<C-o>` goes back to each row once.
//! - `<C-o>` goes `count` entries back (towards older jumps) and `<C-i>`
//!   forward, each to the entry's exact position (clamped as a caret is).
//!   The first `<C-o>` after a jump first adds where it goes from, in place
//!   of the newest entry if that's on its row, and sets `''` there, so
//!   `<C-i>` comes back to it; the others leave `''` alone. Past either end
//!   of the list they fail with nothing changed.
//!
//! [`VimState`]: crate::VimState

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_text::Bias;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    MoveArgs, Pending, Step, VimCommands, VimKey, VimTarget, args::Motion, feature::Feature, marks,
    motion, reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    register,
    ..Feature::NONE
};

/// `vim.jump_older`'s and `vim.jump_newer`'s args.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JumpArgs {
    /// How many entries to go: 1 if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let command = match key {
        VimKey::Ctrl('o') => "vim.jump_older",
        VimKey::Ctrl('i') | VimKey::Tab => "vim.jump_newer",
        _ => return None,
    };
    let taken = !pending.is_visual() && !pending.has_operator() && pending.register().is_none();
    taken.then(|| {
        invocation(
            command,
            JumpArgs {
                count: pending.count(),
            },
        )
    })
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.jump_older",
            "Vim Jump Older",
            |target: &mut VimTarget<'_>, _: &Actor, args: JumpArgs| {
                Ok::<_, CommandError>(run(target, args, true))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Goes `count` entries back through the jumplist, as vim's ctrl-o does, to the entry's exact position. The first after a jump adds where it goes from and sets the jump mark there, so vim.jump_newer comes back. Past the oldest entry it fails with nothing changed.",
        )
        .context(normal.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.jump_newer",
            "Vim Jump Newer",
            |target: &mut VimTarget<'_>, _: &Actor, args: JumpArgs| {
                Ok::<_, CommandError>(run(target, args, false))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Goes `count` entries forward through the jumplist, as vim's ctrl-i and tab do, to the entry's exact position. Past the newest entry it fails with nothing changed.",
        )
        .context(normal)
        .mutates_buffer(false);
}

/// `<C-o>` (`back`) or `<C-i>`.
fn run(target: &mut VimTarget<'_>, args: JumpArgs, back: bool) -> Dispatched<TransactionId> {
    target.model.sync_display();
    let from = target.model.primary_selection().head();
    target.state.marks.watch(target.model);
    let snapshot = target.model.snapshot().clone();
    let count = args.count.unwrap_or(1);
    let (row, column) = match target.state.marks.travel(&snapshot, from, back, count) {
        Some(Some(found)) => found,
        Some(None) => return target.state.fail(marks::INVALID_ROW),
        None => {
            // nvim beeps, with no message.
            target.state.failed = true;
            return Dispatched::new(Effect::Nothing);
        }
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
    let moved = motion::apply(target.model, target.state, args);
    marks::settle(target);
    Dispatched::new(match moved {
        true => Effect::Changed,
        false => Effect::Nothing,
    })
}
