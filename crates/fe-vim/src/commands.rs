//! Vim's commands, on a registry of their own: their handlers need the
//! [`VimState`] (goal columns, the last find), which `fe-editor`'s
//! `EditorTarget` doesn't carry.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, CommandTarget, ContextPredicate, Registry};
use fe_editor::{EditorModel, commands::EditorHost};

use crate::{
    CommandLineArgs, CountArgs, InsertArgs, MoveArgs, OperateArgs, PutArgs, Registers,
    ReplaceCharArgs, SelectArgs, SubmitArgs, SwapArgs, VimState, VisualArgs, cmdline, edit,
    history, insert, motion, operator, repeat, visual,
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
/// `vim.undo`, `vim.redo`, `vim.repeat`, and for the visual modes
/// `vim.visual`, `vim.visual_exit`, `vim.visual_swap`, `vim.visual_put`,
/// `vim.reselect` and `vim.select`, and for the command line (searches and
/// ex commands) `vim.command_line`, `vim.command_line_submit` and
/// `vim.command_line_cancel`.
pub fn registry() -> Registry<VimCommands> {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    let visual_mode =
        ContextPredicate::parse("editor && vim.visual").expect("vim's context predicate parses");
    let normal_or_visual = ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)")
        .expect("vim's context predicate parses");
    let insert_mode = ContextPredicate::parse("editor && vim.mode == insert")
        .expect("vim's context predicate parses");
    let cmdline_mode = ContextPredicate::parse("editor && vim.mode == cmdline")
        .expect("vim's context predicate parses");
    let mut registry = Registry::new();
    registry
        .register_with_args(
            "vim.move",
            "Vim Move",
            |target: &mut VimTarget<'_>, _: &Actor, args: MoveArgs| {
                Ok::<_, CommandError>(motion::move_command(target, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Moves every caret by a vim motion, `count` times or to row `count`; the count is bounded by the text. In a visual mode it moves the vim cursor, keeping the anchor. A search motion's pattern becomes the last search; one that fails moves nothing and leaves a message.",
        )
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.operate",
            "Vim Operate",
            |target: &mut VimTarget<'_>, actor: &Actor, args: OperateArgs| {
                {
                    insert::adopt_closing(target.state);
                    operator::operate(target, actor, args)
                }
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Applies a vim operator (delete, yank, change, indent, outdent) to a motion or text object at every caret, filling the registers as vim does; one transaction for all carets. Change then starts an insert session in the same transaction. In a visual mode it acts on the selection, as do toggle_case, lowercase, uppercase, join and replace.",
        )
        .context(normal_or_visual.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.put",
            "Vim Put",
            |target: &mut VimTarget<'_>, actor: &Actor, args: PutArgs| {
                {
                    insert::adopt_closing(target.state);
                    operator::put(target, actor, args)
                }
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Puts a register after (or before) every caret, `count` times: whole lines below or above the caret's row, a block from the primary caret's column. At most 64 MiB at once.",
        )
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.insert",
            "Vim Insert",
            |target: &mut VimTarget<'_>, actor: &Actor, args: InsertArgs| {
                {
                    insert::adopt_closing(target.state);
                    insert::insert(target, actor, args)
                }
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
                {
                    insert::adopt_closing(target.state);
                    edit::join(target, actor, args)
                }
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
                {
                    insert::adopt_closing(target.state);
                    edit::replace_char(target, actor, args)
                }
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
                {
                    insert::adopt_closing(target.state);
                    edit::toggle_case(target, actor, args)
                }
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
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                history::undo(target, actor, args, false)
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
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                history::undo(target, actor, args, true)
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
        .register_with_args(
            "vim.visual",
            "Vim Visual Mode",
            |target: &mut VimTarget<'_>, _: &Actor, args: VisualArgs| visual::visual(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Enters charwise, linewise or blockwise visual mode at the primary caret, switches to that kind from another, or leaves it if it's the current one.",
        )
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register(
            "vim.visual_exit",
            "Vim Leave Visual Mode",
            |target: &mut VimTarget<'_>, _: &Actor| visual::exit(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description("Leaves visual mode, keeping its extent for `gv`, `'<` and `'>`.")
        .context(visual_mode.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.visual_swap",
            "Vim Swap Selection Ends",
            |target: &mut VimTarget<'_>, _: &Actor, args: SwapArgs| visual::swap(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Moves the visual cursor to the other end of the selection; in a block with `same_row`, to the other corner on its row.",
        )
        .context(visual_mode.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.visual_put",
            "Vim Put Over Selection",
            |target: &mut VimTarget<'_>, actor: &Actor, args: PutArgs| {
                {
                    insert::adopt_closing(target.state);
                    visual::put(target, actor, args)
                }
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Replaces the visual selection with a register, `count` times: `p` (`before` unset) leaves what it replaced in the registers, `P` leaves them as they were.",
        )
        .context(visual_mode.clone())
        .mutates_buffer(true);
    registry
        .register(
            "vim.reselect",
            "Vim Reselect",
            |target: &mut VimTarget<'_>, _: &Actor| visual::reselect(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Selects the last visual extent again, in its kind; in visual mode the current extent becomes the last.",
        )
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.select",
            "Vim Select Text Object",
            |target: &mut VimTarget<'_>, _: &Actor, args: SelectArgs| {
                visual::select_object(target, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Selects a text object in visual mode, or extends the selection by `count` more of it.",
        )
        .context(visual_mode)
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.command_line",
            "Vim Command Line",
            |target: &mut VimTarget<'_>, _: &Actor, args: CommandLineArgs| {
                cmdline::open(target, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Opens a command line of `kind` (`search_forward` for `/`, `search_backward` for `?`, `ex` for `:`): `vim.mode` becomes `cmdline` until the host submits or cancels the line it owns. A search line carries the pending count, register and operator to the submit. An ex line leaves a visual mode (setting `'<` and `'>`) and starts with `'<,'>` from one, or `.,.+{count-1}` after a count: `VimState::command_line_prefill`.",
        )
        .context(normal_or_visual)
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.command_line_submit",
            "Vim Submit Command Line",
            |target: &mut VimTarget<'_>, actor: &Actor, args: SubmitArgs| {
                cmdline::submit(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Ends the command line with its `text`, back in the mode it came from, and runs it: a search moves to the `count`th match of the pattern (an empty one is the last search's), or applies the pending operator up to it; an ex line runs its command over its range (`:{range}`, `:d`, `:noh`). A line that fails leaves a message and changes nothing else.",
        )
        .context(cmdline_mode.clone())
        .mutates_buffer(true);
    registry
        .register(
            "vim.command_line_cancel",
            "Vim Cancel Command Line",
            |target: &mut VimTarget<'_>, _: &Actor| cmdline::cancel(target.state),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Ends the command line without running it, back in the mode it came from (a visual selection as it was), dropping a pending operator.",
        )
        .context(cmdline_mode)
        .mutates_buffer(false);
    registry
}
