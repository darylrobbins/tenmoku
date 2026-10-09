//! The command line as a mode: `/` and `?` open it, the host owns and edits
//! its text, and submitting it runs what it's for (a search, as a motion or
//! an operator's target).

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect, Invocation, Source};

use crate::{
    CommandLineArgs, CommandLineKind, Mode, Motion, MoveArgs, OperateArgs, Step, SubmitArgs,
    Target, VimKey, VimState, VimTarget,
    args::{Search, SearchPattern},
    insert, motion, operator,
    search::{self, Pattern, PatternError},
};

/// An open command line: what it's for and carries, and the mode it came
/// from, which submitting or cancelling it returns to.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandLine {
    pub(crate) args: CommandLineArgs,
    pub(crate) from: Mode,
}

type Handled = Result<Dispatched<TransactionId>, CommandError>;

/// `vim.command_line`: enters `cmdline` mode from normal or a visual mode,
/// which (and its selection) it keeps to return to.
pub(crate) fn open(state: &mut VimState, args: CommandLineArgs) -> Handled {
    state.message = None;
    state.cmdline = Some(CommandLine {
        args,
        from: state.mode,
    });
    state.mode = Mode::CmdLine(args.kind);
    Ok(Dispatched::new(Effect::Changed))
}

/// The line it ends, back in the mode it came from.
fn close(state: &mut VimState) -> Result<CommandLine, CommandError> {
    let line = state
        .cmdline
        .take()
        .ok_or_else(|| CommandError::new("no command line is open"))?;
    state.mode = line.from;
    Ok(line)
}

/// `vim.command_line_cancel`: back to the mode the line came from, a visual
/// selection as it was, dropping what it carried.
pub(crate) fn cancel(state: &mut VimState) -> Handled {
    close(state)?;
    Ok(Dispatched::new(Effect::Changed))
}

/// `vim.command_line_submit`: back to the mode the line came from, then
/// runs the line there.
pub(crate) fn submit(
    target: &mut VimTarget<'_>,
    actor: &fe_command::Actor,
    args: SubmitArgs,
) -> Handled {
    let line = close(target.state)?;
    match line.args.kind {
        CommandLineKind::SearchForward | CommandLineKind::SearchBackward => {
            search(target, actor, line.args, &args.text)
        }
    }
}

/// A search's line: `vim.move` to the `count`th match (in a visual mode, the
/// vim cursor), or `vim.operate` up to it, as [`check_search`] allows. An
/// empty pattern is the last search's. A line that can't run (E35, an
/// offset) changes nothing else and leaves a [`VimState::message`]. The
/// line closing is at least a [`Effect::Notify`].
fn search(
    target: &mut VimTarget<'_>,
    actor: &fe_command::Actor,
    args: CommandLineArgs,
    text: &str,
) -> Handled {
    let forward = args.kind == CommandLineKind::SearchForward;
    let (pattern, offset) = search::split_search(text, if forward { '/' } else { '?' });
    if let Some(offset) = offset.filter(|offset| !offset.is_empty()) {
        return failed(
            target.state,
            format!("search offsets not supported: {offset}"),
        );
    }
    let pattern = match (pattern.is_empty(), target.registers.last_search()) {
        (false, _) => pattern,
        (true, Some((last, _))) => last.to_string(),
        (true, None) => return failed(target.state, "E35: No previous regular expression"),
    };
    let motion = Motion::Search(Search {
        pattern: SearchPattern::new(&pattern),
        forward,
    });
    let mut dispatched = match args.operator {
        Some(operator) => {
            insert::adopt_closing(target.state);
            operator::operate(
                target,
                actor,
                OperateArgs {
                    operator,
                    target: Target::Motion(motion),
                    count: args.count,
                    register: args.register,
                },
            )?
        }
        None => motion::move_command(
            target,
            MoveArgs {
                motion,
                extend: matches!(target.state.mode, Mode::Visual(_)),
                count: args.count,
            },
        ),
    };
    if dispatched.effect == Effect::Nothing {
        dispatched.effect = Effect::Notify;
    }
    Ok(dispatched)
}

/// Checks a search motion before it runs, wherever it comes from (`/`,
/// `vim.move`, `vim.operate`, `.`): its pattern becomes the last search, as
/// in nvim even if nothing matches or Vim rejects it, but not one fe-vim
/// can't translate. `Err` holds the error for [`VimState::message`]: the
/// pattern's, or E486 if nothing matches from the primary caret (the vim
/// cursor in a visual mode).
pub(crate) fn check_search(target: &mut VimTarget<'_>, search: Search) -> Result<(), String> {
    let pattern = search.pattern.as_str();
    let parsed = match Pattern::parse(pattern) {
        Ok(parsed) => parsed,
        Err(error) => {
            if let PatternError::Invalid(_) = error {
                target.registers.set_search(pattern, search.forward);
            }
            return Err(error.to_string());
        }
    };
    target.registers.set_search(pattern, search.forward);
    target.model.sync_display();
    let from = target
        .state
        .visual_cursor(target.model)
        .unwrap_or_else(|| target.model.primary_selection().head());
    // Past one match, a count goes round: any match will do.
    match parsed.find(target.model.text(), from, search.forward) {
        Some(_) => Ok(()),
        None => Err(format!("E486: Pattern not found: {pattern}")),
    }
}

/// Leaves `message` for the host: what changed is only that.
pub(crate) fn failed(state: &mut VimState, message: impl Into<String>) -> Handled {
    state.message = Some(message.into());
    Ok(Dispatched::new(Effect::Notify))
}

/// [`VimState::feed`] in `cmdline` mode, where the host's line should have
/// taken the key: `<Esc>` cancels, so a host with no line can leave it, and
/// every other key is forwarded.
pub(crate) fn fallback(key: VimKey) -> Step {
    match key {
        VimKey::Esc => Step::Commit(
            Invocation::of("vim.command_line_cancel")
                .expect("vim command IDs are valid")
                .source(Source::Keyboard),
        ),
        _ => Step::Forward,
    }
}
