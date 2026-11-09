//! Vim's commands, on a registry of their own: their handlers need the
//! [`VimState`] (goal columns, the last find), which `fe-editor`'s
//! `EditorTarget` doesn't carry.

use fe_buffer::TransactionId;
use fe_command::{
    Actor, CommandError, CommandTarget, ContextPredicate, DispatchError, Dispatched, Effect,
    Invocation, Registry,
};
use fe_editor::{EditorModel, commands::EditorHost};

use crate::{
    CancelArgs, CommandLineArgs, CountArgs, InsertArgs, MarkArgs, MoveArgs, OperateArgs, PlayArgs,
    PutArgs, RecordArgs, Registers, ReplaceCharArgs, SelectArgs, SubmitArgs, SwapArgs, VimState,
    VisualArgs, cmdline, edit, feature, history, insert, jump, line_motions, macros, marks, motion,
    operator, remap, repeat, replace, smart_select, substitute, visual,
};

/// Catches the marks up with the text before a command that may change it,
/// following what it changes (see `marks::follow`).
pub(crate) fn follow(target: &mut VimTarget<'_>) {
    marks::follow(target);
}

/// What a committed `vim.move` runs.
pub(crate) fn run_move(target: &mut VimTarget<'_>, args: MoveArgs) -> Dispatched<TransactionId> {
    if let Some(failed) = jump::check_pick(target) {
        return failed;
    }
    let moved = motion::move_command(target, args);
    marks::settle(target);
    moved
}

/// What a committed `vim.operate` runs.
pub(crate) fn run_operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if let Some(failed) = jump::check_pick(target) {
        return Ok(failed);
    }
    follow(target);
    insert::adopt_closing(target.state);
    let operated = operator::operate(target, actor, args);
    marks::settle(target);
    operated
}

/// Runs `invocation` through `registry`, as a host runs each of vim's
/// commands: [`Registry::dispatch`], then, if the command was the one
/// `<C-o>` left insert mode for and it's done, back to insert mode
/// ([`VimState::one_command`]), whether it worked or not. Going back
/// changes the mode and may move the carets, so a command that did nothing
/// itself (`h` at column 0) still has the view painted again. The marks from
/// before a command go with the transactions it made, for `u` and `<C-r>`
/// ([`history::Moves`]).
pub fn dispatch(
    registry: &Registry<VimCommands>,
    target: &mut VimTarget<'_>,
    invocation: &Invocation,
) -> Result<Dispatched<TransactionId>, DispatchError> {
    let mutates = registry
        .get(&invocation.command)
        .is_some_and(|meta| meta.mutates_buffer);
    let before = history::before(target, mutates);
    let mut dispatched = registry.dispatch(target, invocation);
    let made = match &dispatched {
        Ok(dispatched) => dispatched.transactions.as_slice(),
        Err(_) => &[],
    };
    history::after(target, before, made);
    if target.state.one_command.is_some() {
        // The session this goes back to is a change of its own, after the
        // command's.
        let before = history::before(target, false);
        insert::finish_one_command(target);
        history::after(target, before, &[]);
        if target.state.one_command.is_none()
            && let Ok(dispatched) = &mut dispatched
            && dispatched.effect == Effect::Nothing
        {
            dispatched.effect = Effect::Changed;
        }
    }
    dispatched
}

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
/// `vim.normal_mode`, `vim.resume_insert`, `vim.replace_mode`, `vim.virtual_replace_mode`, `vim.replace_normal_mode`, `vim.join`, `vim.replace_char`, `vim.toggle_case`,
/// `vim.undo`, `vim.redo`, `vim.repeat`, `vim.repeat_substitute`,
/// `vim.substitute_answer`, the
/// macros' `vim.record`, `vim.record_stop` and `vim.play`, and for
/// the visual modes
/// `vim.visual`, `vim.visual_exit`, `vim.visual_swap`, `vim.visual_put`,
/// `vim.reselect` and `vim.select`, and for the command line (searches and
/// ex commands) `vim.command_line`, `vim.command_line_submit` and
/// `vim.command_line_cancel`, and a map's `<Cmd>` ex line's `vim.ex_line`.
/// Feature modules add their commands after these (see [`crate::feature`]).
pub fn registry() -> Registry<VimCommands> {
    let normal = ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == normal"))
        .expect("vim's context predicate parses");
    let visual_mode = ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.visual"))
        .expect("vim's context predicate parses");
    let normal_or_visual = ContextPredicate::parse(kiln_l10n::verbatim!(
        "editor && (vim.mode == normal || vim.visual)"
    ))
    .expect("vim's context predicate parses");
    let insert_mode = ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == insert"))
        .expect("vim's context predicate parses");
    let replace_mode =
        ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == replace"))
            .expect("vim's context predicate parses");
    let cmdline_mode =
        ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == cmdline"))
            .expect("vim's context predicate parses");
    let mut registry = Registry::new();
    registry
        .register_with_args(
            "vim.move",
            &crate::l10n::command_title("vim.move"),
            |target: &mut VimTarget<'_>, _: &Actor, args: MoveArgs| {
                let motion = line_motions::moved(&args);
                line_motions::with_view(target, motion, |target| {
                    Ok::<_, CommandError>(run_move(target, args))
                })
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.move"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.operate",
            &crate::l10n::command_title("vim.operate"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: OperateArgs| {
                let motion = line_motions::operated(&args);
                line_motions::with_view(target, motion, |target| run_operate(target, actor, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.operate"))
        .context(normal_or_visual.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.put",
            &crate::l10n::command_title("vim.put"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: PutArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                operator::put(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.put"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.insert",
            &crate::l10n::command_title("vim.insert"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: InsertArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                insert::insert(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.insert"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register(
            kiln_l10n::verbatim!("vim.normal_mode"),
            &crate::l10n::command_title("vim.normal_mode"),
            |target: &mut VimTarget<'_>, _: &Actor| {
                follow(target);
                insert::normal_mode(target)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.normal_mode"))
        .context(insert_mode)
        .mutates_buffer(true);
    registry
        .register(
            kiln_l10n::verbatim!("vim.resume_insert"),
            &crate::l10n::command_title("vim.resume_insert"),
            |target: &mut VimTarget<'_>, _: &Actor| {
                insert::finish_one_command(target);
                Ok(Dispatched::new(Effect::Changed))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.resume_insert"))
        .context(
            ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.insert_pending"))
                .expect("vim's context predicate parses"),
        )
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.replace_mode",
            &crate::l10n::command_title("vim.replace_mode"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                replace::replace(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.replace_mode"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.virtual_replace_mode",
            &crate::l10n::command_title("vim.virtual_replace_mode"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                replace::virtual_replace(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description(
            "vim.virtual_replace_mode",
        ))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register(
            kiln_l10n::verbatim!("vim.replace_normal_mode"),
            &crate::l10n::command_title("vim.replace_normal_mode"),
            |target: &mut VimTarget<'_>, _: &Actor| {
                follow(target);
                insert::normal_mode(target)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.replace_normal_mode"))
        .context(replace_mode)
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.join",
            &crate::l10n::command_title("vim.join"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                edit::join(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.join"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.insert_last",
            &crate::l10n::command_title("vim.insert_last"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                insert::insert_last(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.insert_last"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.replace_char",
            &crate::l10n::command_title("vim.replace_char"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: ReplaceCharArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                edit::replace_char(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.replace_char"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.toggle_case",
            &crate::l10n::command_title("vim.toggle_case"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                edit::toggle_case(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.toggle_case"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.undo",
            &crate::l10n::command_title("vim.undo"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                history::undo(target, actor, args, false)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.undo"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.redo",
            &crate::l10n::command_title("vim.redo"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                history::undo(target, actor, args, true)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.redo"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.repeat",
            &crate::l10n::command_title("vim.repeat"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                let motion = line_motions::repeated(target.state, args);
                line_motions::with_view(target, motion, |target| {
                    follow(target);
                    let repeated = repeat::repeat(target, actor, args);
                    marks::settle(target);
                    repeated
                })
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.repeat"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.repeat_substitute",
            &crate::l10n::command_title("vim.repeat_substitute"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: CountArgs| {
                follow(target);
                let repeated = substitute::repeat(target, actor, args.count);
                marks::settle(target);
                repeated
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.repeat_substitute"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.substitute_answer",
            &crate::l10n::command_title("vim.substitute_answer"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: substitute::AnswerArgs| {
                follow(target);
                let answered = substitute::answer(target, actor, args);
                marks::settle(target);
                answered
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.substitute_answer"))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register(
            kiln_l10n::verbatim!("vim.substitute_all_lines"),
            &crate::l10n::command_title("vim.substitute_all_lines"),
            |target: &mut VimTarget<'_>, actor: &Actor| {
                follow(target);
                let ran = crate::ex::run(target, actor, "%s//~/&");
                marks::settle(target);
                ran
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description(
            "vim.substitute_all_lines",
        ))
        .context(normal.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.record",
            &crate::l10n::command_title("vim.record"),
            |target: &mut VimTarget<'_>, _: &Actor, args: RecordArgs| {
                macros::record(target.state, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.record"))
        .context(normal.clone())
        .mutates_buffer(false);
    registry
        .register(
            kiln_l10n::verbatim!("vim.record_stop"),
            &crate::l10n::command_title("vim.record_stop"),
            |target: &mut VimTarget<'_>, _: &Actor| macros::stop(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.record_stop"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.play",
            &crate::l10n::command_title("vim.play"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: PlayArgs| {
                macros::play(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.play"))
        .context(normal)
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.set_mark",
            &crate::l10n::command_title("vim.set_mark"),
            |target: &mut VimTarget<'_>, _: &Actor, args: MarkArgs| marks::set(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.set_mark"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.visual",
            &crate::l10n::command_title("vim.visual"),
            |target: &mut VimTarget<'_>, _: &Actor, args: VisualArgs| visual::visual(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.visual"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register(
            kiln_l10n::verbatim!("vim.visual_exit"),
            &crate::l10n::command_title("vim.visual_exit"),
            |target: &mut VimTarget<'_>, _: &Actor| visual::exit(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.visual_exit"))
        .context(visual_mode.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.visual_swap",
            &crate::l10n::command_title("vim.visual_swap"),
            |target: &mut VimTarget<'_>, _: &Actor, args: SwapArgs| visual::swap(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.visual_swap"))
        .context(visual_mode.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.visual_put",
            &crate::l10n::command_title("vim.visual_put"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: PutArgs| {
                {
                    follow(target);
                    insert::adopt_closing(target.state);
                    // The selection goes as a delete takes it, whatever the
                    // register puts back; a put refused leaves the marks.
                    let marks = target.state.marks.clone();
                    match visual::replaced(target.model, target.state) {
                        Some(visual::Replaced::Chars(range)) => {
                            target.state.marks.cutting(target.model.snapshot(), range);
                        }
                        Some(visual::Replaced::Rows(first, last)) => {
                            target
                                .state
                                .marks
                                .deleting(target.model.snapshot(), first, last);
                        }
                        None => {}
                    }
                    let put = visual::put(target, actor, args);
                    if put.is_err() {
                        target.state.marks = marks;
                    }
                    put
                }
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.visual_put"))
        .context(visual_mode.clone())
        .mutates_buffer(true);
    registry
        .register(
            kiln_l10n::verbatim!("vim.reselect"),
            &crate::l10n::command_title("vim.reselect"),
            |target: &mut VimTarget<'_>, _: &Actor| visual::reselect(target),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.reselect"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.select",
            &crate::l10n::command_title("vim.select"),
            |target: &mut VimTarget<'_>, _: &Actor, args: SelectArgs| {
                let object = line_motions::selected(&args);
                line_motions::with_view(target, object, |target| {
                    visual::select_object(target, args)
                })
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.select"))
        .context(visual_mode)
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.command_line",
            &crate::l10n::command_title("vim.command_line"),
            |target: &mut VimTarget<'_>, _: &Actor, args: CommandLineArgs| {
                cmdline::open(target, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.command_line"))
        .context(normal_or_visual.clone())
        .mutates_buffer(false);
    registry
        .register_with_args(
            "vim.command_line_submit",
            &crate::l10n::command_title("vim.command_line_submit"),
            |target: &mut VimTarget<'_>, actor: &Actor, args: SubmitArgs| {
                follow(target);
                let submitted = cmdline::submit(target, actor, args);
                marks::settle(target);
                submitted
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.command_line_submit"))
        .context(cmdline_mode.clone())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "vim.command_line_cancel",
            &crate::l10n::command_title("vim.command_line_cancel"),
            |target: &mut VimTarget<'_>, _: &Actor, args: CancelArgs| cmdline::cancel(target, args),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.command_line_cancel"))
        .context(cmdline_mode)
        .mutates_buffer(false);
    registry
        .register_with_args(
            remap::EX_LINE,
            &crate::l10n::command_title(remap::EX_LINE),
            |target: &mut VimTarget<'_>, actor: &Actor, args: remap::ExLineArgs| {
                follow(target);
                let ran = remap::ex_line(target, actor, args);
                marks::settle(target);
                ran
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description(remap::EX_LINE))
        .context(
            ContextPredicate::parse(kiln_l10n::verbatim!("editor"))
                .expect("vim's context predicate parses"),
        )
        .mutates_buffer(true);
    registry
        .register(
            crate::ex::normal::RESUME,
            &crate::l10n::command_title(crate::ex::normal::RESUME),
            |target: &mut VimTarget<'_>, _: &Actor| {
                follow(target);
                let ran = crate::ex::normal::resume(target);
                marks::settle(target);
                ran
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description(crate::ex::normal::RESUME))
        .context(
            ContextPredicate::parse(kiln_l10n::verbatim!("editor"))
                .expect("vim's context predicate parses"),
        )
        .mutates_buffer(true);
    smart_select::register(&mut registry, normal_or_visual);
    feature::register(&mut registry);
    registry
}
