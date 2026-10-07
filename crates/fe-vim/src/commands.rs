//! Vim's commands, on a registry of their own: their handlers need the
//! [`VimState`] (goal columns, the last find), which `fe-editor`'s
//! `EditorTarget` doesn't carry.

use fe_buffer::TransactionId;
use fe_command::{
    Actor, CommandError, CommandTarget, ContextPredicate, Dispatched, Effect, Registry,
};
use fe_editor::{EditorModel, commands::EditorHost};

use crate::{MoveArgs, OperateArgs, PutArgs, Registers, VimState, motion, operator};

/// What vim's commands run against: an editor's model, its host, its vim
/// state and the host's registers, borrowed for one dispatch.
pub struct VimTarget<'a> {
    pub model: &'a mut EditorModel,
    pub host: &'a mut dyn EditorHost,
    pub state: &'a mut VimState,
    /// Shared by every editor in the host.
    pub registers: &'a mut Registers,
}

/// The [`CommandTarget`] of vim's [`Registry`].
pub struct VimCommands;

impl CommandTarget for VimCommands {
    type Tx = TransactionId;
    type Target<'a> = VimTarget<'a>;
}

/// Vim's registry: `vim.move`, `vim.operate` and `vim.put`.
pub fn registry() -> Registry<VimCommands> {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    let mut registry = Registry::new();
    registry
        .register_with_args(
            "vim.move",
            "Vim Move",
            |target: &mut VimTarget<'_>, _: &Actor, args: MoveArgs| {
                let moved = motion::apply(target.model, target.state, args);
                Ok::<_, CommandError>(Dispatched::new(match moved {
                    true => Effect::Changed,
                    false => Effect::Nothing,
                }))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Moves every caret by a vim motion, `count` times or to row `count`; the count is bounded by the text.",
        )
        .context(normal.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.operate",
            "Vim Operate",
            |target: &mut VimTarget<'_>, actor: &Actor, args: OperateArgs| {
                operator::operate(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Applies a vim operator (delete, yank, indent, outdent) to a motion or text object at every caret, filling the registers as vim does; one transaction for all carets.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.put",
            "Vim Put",
            |target: &mut VimTarget<'_>, actor: &Actor, args: PutArgs| {
                operator::put(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Puts a register after (or before) every caret, `count` times: whole lines below or above the caret's row. At most 64 MiB at once.",
        )
        .context(normal)
        .mutates_buffer(true);
    registry
}
