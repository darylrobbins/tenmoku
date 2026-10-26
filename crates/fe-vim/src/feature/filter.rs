//! `!` (a feature module; see [`crate::feature`]): `!{motion}` and
//! `!{object}`, `{count}!!` and `{Visual}!` ([`Operator::Filter`]) open
//! the command line on the rows they cover, prefilled as nvim's are, for a
//! filter through a shell command (`ex/filter.rs`). The caret first goes
//! where nvim's goes, the start of what the operator covers: where the
//! motion lands if that's before the caret (`!!` at the row's first
//! non-blank), a whole-row object's (`ip`) first row's start, and a jump
//! motion (`!G`) sets the jump mark; the line then starts `.!` for one
//! row, `.,$!` for rows to the last, `.,.+{n}!` for others, and `'<,'>!`
//! from a visual mode, which it leaves as `:` does. It carries nothing
//! else.
//!
//! Submitted from the keys that opened it, with the range they gave, the
//! line's filter is what `.` repeats ([`Change::Filter`]): the operator
//! again over its motion or object at the caret, or over as many rows as
//! the selection had, with the same command and no line, refused as the
//! line would be for any actor but the user and inside `:g` or `:normal`
//! (`vim.filter`, which runs it, is no different).
//!
//! `!gc` selects the run of commented rows around the caret, as nvim's
//! object (a visual selection) does, and opens `'<,'>!`; `.` finds the run
//! around the caret again, whatever its size, and off a comment does
//! nothing.
//!
//! Known limits: `.` after a visual `!` takes as many rows
//! as the selection had, as fe-vim's `.` after any visual change does; and
//! a macro's replay runs no shell command, through the maps it runs too
//! (`ex/filter.rs`), so a filter in a register asks for nothing, where
//! nvim's runs.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Change, CommandLineArgs, CommandLineKind, Motion, MoveArgs, OperateArgs, Operator, Pending,
    Step, Target, TextObject, VimCommands, VimKey, VimTarget, cmdline, commands,
    ex::{self, HostCommand, ShellCommand},
    feature::{Feature, comment},
    folds, insert, line_motions, marks,
    motion::{self, Cursor},
    operator::{self, Span},
    text_object, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    register,
    ..Feature::NONE
};

const OPERATOR: Operator = Operator::Filter;

/// `vim.filter`'s args, and what `.` repeats after a filter: `command`
/// over the rows `target` (a motion, an object, or [`Motion::Line`]) covers
/// at the caret, with `count`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilterArgs {
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    pub command: ShellCommand,
}

/// What the feature keeps between commands.
#[derive(Clone, Debug, Default)]
pub(crate) struct Filters {
    /// The command line the keys last opened, until a line runs.
    pub(crate) opened: Option<Opened>,
    /// The line running came from it: what `.` repeats it over.
    submitted: Option<(Target, Option<u32>)>,
    /// The last shell command a line ran (`:.!!` repeats it), nvim's
    /// `prevcmd`.
    pub(crate) last: Option<ShellCommand>,
    /// While `.` finds a `gc` object's run again ([`comment_run`]): the
    /// rows `comment::operate_object` selected for [`operate`], which notes
    /// them here, leaving the visual mode, instead of opening a line.
    finding: Option<Option<(u32, u32)>>,
}

/// A command line `!` opened: what over, the text it started with and the
/// row the caret was on.
#[derive(Clone, Debug)]
pub(crate) struct Opened {
    target: Target,
    count: Option<u32>,
    prefill: String,
    row: u32,
}

/// `!`: starts the operator, or with it pending takes `count` rows (`!!`);
/// in a visual mode it covers the selection.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('!') {
        return None;
    }
    let pending = pending.clone();
    match pending.operator() {
        _ if pending.is_visual() => Some(pending.operate(OPERATOR, Target::Selection)),
        None => Some(pending.with_operator(OPERATOR)),
        Some(OPERATOR) => Some(pending.operate(OPERATOR, Target::Motion(Motion::Line))),
        Some(_) => None,
    }
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.filter",
            "Vim Filter",
            |target: &mut VimTarget<'_>, actor: &Actor, args: FilterArgs| {
                commands::follow(target);
                insert::adopt_closing(target.state);
                run(target, actor, &args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Asks the host to run `command` (a shell command, in parts; none for the last one, E34 with none) over the rows `target` covers at the caret with `count`, as `.` repeats `!{motion}`: leaves `HostCommand::Filter`, whose output `VimState::apply_filter` puts in their place. Only for the user's own keys: any other actor, or a `:g` or `:normal` running it, gets `Shell commands run only from your own keys` and no host command.",
        )
        .context(normal)
        .mutates_buffer(true);
}

/// The rows `target` covers at the primary caret, with `count`, and where
/// nvim's operator puts the caret before it acts; `None` if it covers none
/// (a failed motion, which nvim may still move the caret for: there).
fn covered(
    target: &mut VimTarget<'_>,
    over: Target,
    count: Option<u32>,
) -> Result<(u32, u32, usize), Option<usize>> {
    // `n N * #` as the search they are, and a mark's motion at the mark.
    let over = match over {
        Target::Motion(motion) => {
            let motion = cmdline::resolve(target, motion).map_or(motion, |(resolved, _)| resolved);
            Target::Motion(marks::resolve(target, motion).unwrap_or(motion))
        }
        other => other,
    };
    let jump = match over {
        Target::Motion(motion) => marks::jump(target, motion, count),
        _ => None,
    };
    operator::sync(target.model);
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let head = Cursor::at(&text, target.model.primary_selection().head()).offset();
    let state = &mut *target.state;
    let last_find = state.last_find;
    let span = match over {
        Target::Motion(motion) => {
            if let Motion::FindForward(_)
            | Motion::FindBackward(_)
            | Motion::TillForward(_)
            | Motion::TillBackward(_) = motion
            {
                state.last_find = Some(motion);
            }
            operator::motion_span(&text, head, motion, count, last_find, tab)
        }
        Target::TextObject(object) => {
            text_object::select(&text, head, object, count.unwrap_or(1), tab)
        }
        Target::Selection | Target::SelectedLines | Target::SelectedToEnd => Span::Failed,
    };
    let row = |offset: usize| text.offset_to_point(offset).row;
    if let (Some(from), Span::Chars(_) | Span::Lines { .. }) = (jump, &span) {
        state.marks.jumped(from);
    }
    match span {
        Span::Failed => Err(None),
        Span::Move(offset) => Err(Some(Cursor::at(&text, offset).offset())),
        Span::Chars(range) => Ok((
            row(range.start),
            row(range.end.saturating_sub(1).max(range.start)),
            range.start,
        )),
        Span::Lines { rows, .. } => {
            let (first, last) = (*rows.start(), *rows.end());
            // A motion's start, as nvim's operator takes it: where it
            // lands, if before the caret (`!!` and `_` at the first
            // non-blank, `!k` on the row above); an object's first row's.
            let start = match over {
                Target::Motion(motion) => {
                    let mut cursor = Cursor::at(&text, head);
                    cursor.op = true;
                    let args = crate::MoveArgs {
                        motion,
                        extend: false,
                        count,
                    };
                    match motion::motion(&mut cursor, args, last_find, None, tab) {
                        Ok(_) if row(cursor.offset().min(head)) == first => {
                            cursor.offset().min(head)
                        }
                        _ => text.row_start(first),
                    }
                }
                _ => text.row_start(first),
            };
            Ok((first, last, start))
        }
    }
}

/// The primary caret's row.
pub(crate) fn caret_row(target: &VimTarget<'_>) -> u32 {
    let model = &target.model;
    model
        .text()
        .offset_to_point(model.primary_selection().head())
        .row
}

/// One caret at `at`, its goal reset if it moved.
fn put_caret(target: &mut VimTarget<'_>, at: usize) {
    if target.model.primary_selection().head() != at || target.model.selections().len() > 1 {
        target.model.set_selections([operator::caret(at)]);
        target.state.goals = None;
    }
}

/// A failed target, which leaves the caret where it `moved` to, if
/// anywhere, as nvim's operator does.
fn failed(target: &mut VimTarget<'_>, moved: Option<usize>) -> Dispatched<TransactionId> {
    let before = target.model.selections();
    if let Some(at) = moved {
        put_caret(target, at);
    }
    target.state.failed = true;
    Dispatched::new(match target.model.selections() != before {
        true => Effect::Changed,
        false => Effect::Nothing,
    })
}

/// `vim.operate` with `!`: the command line opened on the rows it covers,
/// as the module describes; a failed motion opens none.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let line = CommandLineArgs {
        kind: CommandLineKind::Ex,
        count: None,
        register: None,
        operator: None,
    };
    let (repeat, count, suffix) = match args.target {
        Target::Selection | Target::SelectedLines | Target::SelectedToEnd
            if target.state.filter.finding.is_some() =>
        {
            visual::exit(target)?;
            let snapshot = target.model.snapshot();
            let row = |name| {
                let mark = target.state.mark(name)?;
                Some(mark.get(snapshot)?.0)
            };
            let run = row('<')
                .zip(row('>'))
                .map(|(first, last)| (first.min(last), first.max(last)));
            target.state.filter.finding = Some(run);
            return Ok(Dispatched::new(Effect::Changed));
        }
        Target::Selection | Target::SelectedLines | Target::SelectedToEnd => {
            // Leaving the visual mode sets `'<` and `'>`, and the line
            // starts with them.
            cmdline::open(target, line)?;
            let snapshot = target.model.snapshot();
            let row = |name| {
                let mark = target.state.mark(name)?;
                Some(mark.get(snapshot)?.0)
            };
            let rows = match (row('<'), row('>')) {
                (Some(first), Some(last)) => last.abs_diff(first) + 1,
                _ => 1,
            };
            (Target::Motion(Motion::Line), Some(rows), "!".to_string())
        }
        over => {
            let (first, last, start) = match covered(target, over, args.count) {
                Ok(covered) => covered,
                Err(moved) => return Ok(failed(target, moved)),
            };
            put_caret(target, start);
            let end = target.model.text().row_count() - 1;
            let prefill = match last - first {
                0 => ".!".to_string(),
                _ if last == end => ".,$!".to_string(),
                more => format!(".,.+{more}!"),
            };
            cmdline::open(target, line)?;
            (over, args.count, prefill)
        }
    };
    let row = caret_row(target);
    let state = &mut *target.state;
    let Some(opened) = state.cmdline.as_mut() else {
        return Ok(Dispatched::new(Effect::Changed));
    };
    opened.prefill.push_str(&suffix);
    state.filter.opened = Some(Opened {
        target: repeat,
        count,
        prefill: opened.prefill.clone(),
        row,
    });
    Ok(Dispatched::new(Effect::Changed))
}

/// `!gc`: the `gc` object's run selected, as `comment::operate_object`
/// selects it for any operator (nvim's object is a visual selection too:
/// `'<,'>!`), the line it opens noting the object, which `.` finds again
/// around the caret ([`comment_run`]), whatever its size.
pub(crate) fn operate_object(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.state.filter.opened = None;
    let dispatched = comment::operate_object(target, actor, args)?;
    if target.state.cmdline.is_some()
        && let Some(opened) = target.state.filter.opened.as_mut()
    {
        opened.target = args.target;
        opened.count = args.count;
    }
    Ok(dispatched)
}

/// The `gc` object's run around the caret again, for `.` repeating `args`
/// as `actor`: selected as the `!gc` selected it, then left; `None` off a
/// comment or with no comment tokens.
fn comment_run(target: &mut VimTarget<'_>, actor: &Actor, args: &FilterArgs) -> Option<(u32, u32)> {
    let operate = OperateArgs {
        operator: OPERATOR,
        target: args.target,
        count: args.count,
        register: None,
    };
    target.state.filter.finding = Some(None);
    let ran = comment::operate_object(target, actor, operate);
    let found = target.state.filter.finding.take().flatten();
    ran.ok().and(found)
}

/// Before an ex `line` runs, with the caret on `row`: whether it's the one
/// `!` opened, still starting as it did, on the row it was opened on; then
/// the filter it runs is what `.` repeats. Any line drops the one opened.
pub(crate) fn submitting(state: &mut crate::VimState, line: &str, row: u32) {
    let opened = state.filter.opened.take();
    let line = line.trim_start_matches([':', ' ', '\t']);
    state.filter.submitted = opened
        .filter(|opened| opened.row == row && line.starts_with(&opened.prefill))
        .map(|opened| (opened.target, opened.count));
}

/// The filter of `command` ran from the line `!` opened, if it did: `.`
/// repeats it.
pub(crate) fn submitted(state: &mut crate::VimState, command: &ShellCommand) {
    if let Some((target, count)) = state.filter.submitted.take() {
        let command = command.clone();
        let args = FilterArgs {
            target,
            count,
            command,
        };
        state.set_last_change(Change::Filter(args).into());
    }
}

/// The line has run.
pub(crate) fn ran(state: &mut crate::VimState) {
    state.filter.submitted = None;
}

/// `vim.filter`, and `.` after a filter: [`HostCommand::Filter`] of `args`'
/// command over the rows its target covers at the caret, which goes to
/// their start, or for any actor but the user (or inside `:g` or
/// `:normal`) the refusal and no host command.
pub(crate) fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: &FilterArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.state.host_command = None;
    if let Err(refused) = ex::shell_refusal(target.state, actor) {
        return Ok(target.state.fail(refused.to_string()));
    }
    // Its motion made ready as `vim.operate` makes it: `gj`, `gk` and `g$`
    // over display rows, `zj` and `zk` among the host's folds and those
    // closed; or `af` and the like, the host's syntax around the carets.
    let motion = match args.target {
        Target::Motion(motion) => Some(motion),
        _ => None,
    };
    if let Some(motion) = motion {
        insert::began_line(target.state, motion);
    }
    let viewed = match motion {
        Some(motion) => line_motions::moved(&MoveArgs {
            motion,
            extend: false,
            count: args.count,
        }),
        None => line_motions::object(args.target),
    };
    line_motions::with_view(target, viewed, |target| {
        folds::with_candidates(target, motion, |target| run_over(target, actor, args))
    })
}

/// [`run`] once its motion is ready.
fn run_over(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: &FilterArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    // A search target is checked, and becomes the last search, as
    // `vim.operate` has it; a mark's must be set.
    if let Target::Motion(motion) = args.target {
        let checked = cmdline::resolve(target, motion)
            .and_then(|(motion, keep)| cmdline::check_motion_search(target, motion, keep));
        if let Err(message) = checked.and_then(|()| marks::resolve(target, motion).map(drop)) {
            return cmdline::failed(target.state, message);
        }
    }
    // No command is the last, as on the line.
    let mut args = args.clone();
    if args.command.is_empty() {
        match target.state.filter.last.clone() {
            Some(last) => args.command = last,
            None => {
                return Ok(target
                    .state
                    .fail(ex::ExError::NoPreviousCommand.to_string()));
            }
        }
    }
    let covered = match args.target {
        Target::TextObject(TextObject::Comment) => comment_run(target, actor, &args)
            .map(|(first, last)| (first, last, target.model.text().row_start(first)))
            .ok_or(None),
        over => covered(target, over, args.count),
    };
    let (first, last, start) = match covered {
        Ok(covered) => covered,
        Err(moved) => return Ok(failed(target, moved)),
    };
    put_caret(target, start);
    let state = &mut *target.state;
    state.filter.last = Some(args.command.clone());
    state.host_command = Some(HostCommand::Filter {
        rows: first..last + 1,
        command: args.command.clone(),
        actor: actor.clone(),
    });
    state.set_last_change(Change::Filter(args).into());
    Ok(Dispatched::new(Effect::Changed))
}
