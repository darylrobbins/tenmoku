//! Vim's commands, on a registry of their own: their handlers need the
//! [`VimState`] (goal columns, the last find), which `fe-editor`'s
//! `EditorTarget` doesn't carry.

use fe_buffer::TransactionId;
use fe_command::{
    Actor, CommandError, CommandTarget, ContextPredicate, Dispatched, Effect, Registry,
};
use fe_editor::{EditorModel, commands::EditorHost};

use crate::{
    CountArgs, InsertArgs, MoveArgs, OperateArgs, PutArgs, Registers, ReplaceCharArgs, VimState,
    edit, history, insert, motion, operator, repeat,
};

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

/// Vim's registry: `vim.move`, `vim.operate`, `vim.put`, `vim.insert`,
/// `vim.normal_mode`, `vim.join`, `vim.replace_char`, `vim.toggle_case`,
/// `vim.undo`, `vim.redo` and `vim.repeat`.
pub fn registry() -> Registry<VimCommands> {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    let insert_mode = ContextPredicate::parse("editor && vim.mode == insert")
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
            "Applies a vim operator (delete, yank, change, indent, outdent) to a motion or text object at every caret, filling the registers as vim does; one transaction for all carets. Change then starts an insert session in the same transaction.",
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
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.insert",
            "Vim Insert",
            |target: &mut VimTarget<'_>, actor: &Actor, args: InsertArgs| {
                insert::insert(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Enters insert mode before or after the caret, at the row's first non-blank or end, or on a new row below or above: one transaction until normal mode, typed text going in `count` times.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register(
            "vim.normal_mode",
            "Vim Normal Mode",
            |target: &mut VimTarget<'_>, _: &Actor| insert::normal_mode(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Leaves insert mode: repeats the typed text for the count, ends the session's transaction and steps the caret back one grapheme.",
        )
        .context(insert_mode)
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.join",
            "Vim Join Lines",
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                edit::join(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Joins `count` rows (at least two) from each caret's, dropping indentation and putting in one space where vim does.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.replace_char",
            "Vim Replace Character",
            |target: &mut VimTarget<'_>, actor: &Actor, args: ReplaceCharArgs| {
                edit::replace_char(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Replaces `count` graphemes from each caret with a character, if that many remain on the row.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.toggle_case",
            "Vim Toggle Case",
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                edit::toggle_case(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Toggles the case of `count` graphemes from each caret, as nvim's `~` does, and moves past them.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.undo",
            "Vim Undo",
            |target: &mut VimTarget<'_>, _: &Actor, args: CountArgs| {
                history::undo(target, args, false)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Undoes `count` changes, putting the carets back where they were before the last one undone.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.redo",
            "Vim Redo",
            |target: &mut VimTarget<'_>, _: &Actor, args: CountArgs| {
                history::undo(target, args, true)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Redoes `count` undone changes, putting the carets back where they were before the last one.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.repeat",
            "Vim Repeat",
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                repeat::repeat(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Repeats the last change and the text its insert session typed, as one transaction; a count replaces the change's own.",
        )
        .context(normal)
        .mutates_buffer(true);
    registry
}
