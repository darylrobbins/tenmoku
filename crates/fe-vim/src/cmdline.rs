//! The command line as a mode: `/`, `?` and `:` open it, the host owns and
//! edits its text, and submitting it runs what it's for (a search, as a
//! motion or an operator's target, or an ex command line, [`crate::Ex`]).
//!
//! A search line may end in an offset after the closing delimiter
//! (`:help search-offset`, [`SearchOffset`]): `/foo/e+1`, `/foo/s-1` (or
//! `b-1`), `/foo/+2`, `?foo?e`. `/<CR>` (and `?<CR>`) is the last pattern
//! with the last offset, `//e` the last pattern with a new one, and `//`
//! the last pattern with none. `n` and `N` keep the last offset; `*` and
//! `#` drop it, as an ex address's pattern does, while `:s` and `:g` leave
//! it. A `;` chained search (`/foo/;/bar`) or any other offset text fails
//! with nothing changed. [`offset`] says where a search with an offset
//! lands.

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect, Invocation, Source};

use crate::{
    CommandLineArgs, CommandLineKind, Mode, Motion, MoveArgs, OperateArgs, Pending, SearchOffset,
    Step, SubmitArgs, Target, VimKey, VimState, VimTarget,
    args::{Search, SearchPattern},
    ex, insert, motion, operator,
    search::{self, Pattern},
    visual,
};

/// An open command line: what it's for and carries, the mode it came from,
/// which submitting or cancelling it returns to, and the text it starts
/// with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandLine {
    pub(crate) args: CommandLineArgs,
    pub(crate) from: Mode,
    pub(crate) prefill: String,
}

pub(crate) mod offset;

type Handled = Result<Dispatched<TransactionId>, CommandError>;

/// `vim.command_line`: enters `cmdline` mode from normal or a visual mode.
/// A search line keeps the mode (and its selection) to return to. An ex
/// line returns to normal mode: from a visual mode it leaves it first,
/// setting `'<` and `'>` and putting the caret on `'<`, and starts with
/// `'<,'>`; after a count it starts with `.,.+{count-1}` (`.` for 1).
pub(crate) fn open(target: &mut VimTarget<'_>, args: CommandLineArgs) -> Handled {
    let mut prefill = String::new();
    if args.kind == CommandLineKind::Ex {
        if args.operator.is_some() || args.register.is_some() {
            return Err(CommandError::new(
                "an ex command line takes no operator or register",
            ));
        }
        if visual::leave_for_ex(target) {
            prefill.push_str("'<,'>");
        } else if let Some(count) = args.count.filter(|&count| count > 0) {
            prefill.push('.');
            if count > 1 {
                prefill.push_str(&format!(",.+{}", count - 1));
            }
        }
    }
    let state = &mut *target.state;
    state.message = None;
    // The line's `args` carry the prefix now; nothing of it stays pending.
    state.pending = Pending::new();
    state.cmdline = Some(CommandLine {
        args,
        from: state.mode,
        prefill,
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
/// runs the line there. The line goes into its history first, so one that
/// fails is kept too, as in nvim.
pub(crate) fn submit(
    target: &mut VimTarget<'_>,
    actor: &fe_command::Actor,
    args: SubmitArgs,
) -> Handled {
    let line = close(target.state)?;
    target.registers.add_history(line.args.kind, &args.text);
    match line.args.kind {
        CommandLineKind::SearchForward | CommandLineKind::SearchBackward => {
            search(target, actor, line.args, &args.text)
        }
        CommandLineKind::Ex => ex::run(target, actor, &args.text),
    }
}

/// A search's line: `vim.move` to the `count`th match (in a visual mode, the
/// vim cursor) moved by its offset, or `vim.operate` up to there, as
/// [`check_search`] allows. An empty pattern is the last search's, with the
/// last offset unless the line gives one. A line that can't run (E35, an
/// offset fe-vim doesn't take) changes nothing else and leaves a
/// [`VimState::message`]. The line closing is at least a [`Effect::Notify`].
fn search(
    target: &mut VimTarget<'_>,
    actor: &fe_command::Actor,
    args: CommandLineArgs,
    text: &str,
) -> Handled {
    let forward = args.kind == CommandLineKind::SearchForward;
    let (pattern, offset_text) = search::split_search(text, if forward { '/' } else { '?' });
    let offset = match offset_text.map(offset::parse) {
        None if pattern.is_empty() => target.registers.last_search_offset(),
        None => SearchOffset::None,
        Some(Some(offset)) => offset,
        Some(None) => {
            let text = offset_text.unwrap_or_default();
            return failed(
                target.state,
                format!("search offsets not supported: {text}"),
            );
        }
    };
    let pattern = match (pattern.is_empty(), target.registers.last_search()) {
        (false, _) => pattern,
        (true, Some((last, _))) => last.to_string(),
        (true, None) => return failed(target.state, "E35: No previous regular expression"),
    };
    let motion = Motion::Search(Search {
        pattern: SearchPattern::new(&pattern),
        forward,
        offset,
        again: false,
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
/// `vim.move`, `vim.operate`, `.`): its pattern and offset become the last
/// search, as in nvim even if nothing matches, but not one fe-vim can't translate or
/// one that is invalid, which leave the last search and its direction. `Err` holds the error for [`VimState::message`]: the
/// pattern's, `Interrupted` if the search passed [`search::set_time_limit`]'s
/// limit (the cursor stays; the pattern is still the last search), or E486 if
/// nothing matches from the primary caret (the vim
/// cursor in a visual mode).
pub(crate) fn check_search(
    target: &mut VimTarget<'_>,
    search: Search,
    keep_direction: bool,
) -> Result<(), String> {
    // `n` and `N` keep the direction `/` or `?` gave the search.
    let forward = match (keep_direction, target.registers.last_search()) {
        (true, Some((_, forward))) => forward,
        _ => search.forward,
    };
    let pattern = search.pattern.as_str();
    let parsed = match Pattern::parse(pattern) {
        Ok(parsed) => parsed,
        Err(error) => return Err(error.to_string()),
    };
    target.registers.set_search(pattern, forward);
    target.registers.set_search_offset(search.offset);
    target.state.highlight = Some(parsed.clone());
    let from = caret(target);
    // Past one match, a count goes round: any match will do.
    match parsed.try_find(target.model.text(), from, search.forward) {
        Ok(Some(_)) => Ok(()),
        Err(interrupted) => Err(interrupted.to_string()),
        Ok(None) => Err(format!("E486: Pattern not found: {pattern}")),
    }
}

/// The primary caret, or the vim cursor in a visual mode, with the model
/// caught up.
fn caret(target: &mut VimTarget<'_>) -> usize {
    target.model.sync_display();
    target
        .state
        .visual_cursor(target.model)
        .unwrap_or_else(|| target.model.primary_selection().head())
}

/// `motion` as the search it runs: `n` and `N` are the last search, in its
/// direction or against it, with its offset, and `*` and `#` search for the
/// keyword under or after the caret (or, with none on the rest of the row,
/// the non-blank text there) as the last search, with no offset. `Err` holds the message of one that
/// can't run (E35, E348). Any other motion comes back as it was. The flag
/// is whether the search keeps the last search's direction (`n` and `N`).
pub(crate) fn resolve(
    target: &mut VimTarget<'_>,
    motion: Motion,
) -> Result<(Motion, bool), String> {
    let forward = match motion {
        Motion::SearchNext | Motion::SearchPrevious => {
            let Some((last, was_forward)) = target.registers.last_search() else {
                return Err("E35: No previous regular expression".to_string());
            };
            let forward = was_forward == (motion == Motion::SearchNext);
            let search = Search {
                pattern: SearchPattern::new(last),
                forward,
                offset: target.registers.last_search_offset(),
                again: true,
            };
            return Ok((Motion::Search(search), true));
        }
        Motion::SearchWordForward => true,
        Motion::SearchWordBackward => false,
        _ => return Ok((motion, false)),
    };
    let at = caret(target);
    let Some((_, pattern)) = search::word_at(target.model.text(), at, forward) else {
        return Err("E348: No string under cursor".to_string());
    };
    let search = Search {
        pattern: SearchPattern::new(&pattern),
        forward,
        offset: SearchOffset::None,
        again: false,
    };
    Ok((Motion::SearchWord(search), false))
}

/// Leaves `message` for the host: what changed is only that.
pub(crate) fn failed(state: &mut VimState, message: impl Into<String>) -> Handled {
    Ok(state.fail(message))
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
