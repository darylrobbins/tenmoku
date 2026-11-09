//! `gr` replacing text with a register's (a feature module; see
//! [`crate::feature`]), as the ReplaceWithRegister plugin (v1.42) does:
//! `[count]["x]gr{motion}` replaces what the motion or text object covers,
//! `[count]["x]grr` (or `grgr`) `count` rows, and `{Visual}["x]gr` the
//! selection ([`Operator::ReplaceWithRegister`]). The count goes to the
//! motion or the rows, never to the put: the register goes in once.
//!
//! It takes `gr` only while
//! [`VimState::set_replace_with_register`](crate::VimState::set_replace_with_register)
//! is on (off by default: `gr` is then the host's language-server prefix,
//! and [`VimState::feed`](crate::VimState::feed) cancels it); `vim.operate` with the operator works
//! either way.
//!
//! As the plugin does, it selects what the operator covers (`` g`[vg`] ``,
//! or `V` over rows) and puts the register over it as visual `P` does, so
//! the old text goes to the black hole: no register changes, the unnamed,
//! `"-`, `"0`-`"9` and the clipboard's included. The text, the caret, `'[`,
//! `']`, `'<` and `'>` are visual `P`'s; it's one undo step and leaves the
//! jumplist alone. Before the put the register's text is corrected as the
//! plugin's `s:CorrectForRegtype` does, by what it replaces and not by what
//! the register holds: a linewise register over characters or rows goes in
//! characterwise, without its last line break (so over characters no line
//! break is added, and over rows its lines take the rows); over a block of
//! several rows, a characterwise register's lines (or a linewise one of one
//! line), as Vim's `split()` gives them, are repeated on each row as a
//! block, and a linewise one of several lines goes in as the block
//! `setreg(x, '', "a\<C-v>")` makes of it: its rows and an empty one,
//! 0 columns wide. A correction larger than a put may insert is refused. An
//! empty span (`gri"` on `""`: `'[` after `']`) puts the corrected text
//! before the caret, as `P`.
//!
//! `.` repeats it with the same register, a numbered one too (as
//! repeat.vim does, where a put's `.` takes the next); after a visual `gr`,
//! over as much text at the caret, a block as many columns wide.
//!
//! Known differences from the plugin: the expression register `"=` isn't
//! there to read; 'selection' is always inclusive, so the plugin's
//! exclusive-selection case doesn't arise; a blockwise motion (`gr<C-v>j`)
//! isn't taken; it replaces at the primary caret only; and `.` after a
//! visual `gr` takes as much text as the selection had, as fe-vim's `.`
//! after any visual change does, where the plugin's visualrepeat.vim
//! reselects with `1v`, which nvim sizes by the text the put left.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};

use kiln_l10n::tr;

use crate::{
    Change, Motion, OperateArgs, Operator, Pending, PutArgs, Register, Step, Target, VimKey,
    VimTarget, VisualKind, cmdline,
    feature::Feature,
    marks,
    motion::Cursor,
    operator::{self, Span},
    text_object, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    ..Feature::NONE
};

const OPERATOR: Operator = Operator::ReplaceWithRegister;

/// `r` after `g`: starts `gr`, or with it pending takes `count` rows
/// (`grgr`); in a visual mode it replaces the selection.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('r') {
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

/// The doubled `r` of `grr`.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    (key == VimKey::Char('r') && !pending.is_visual() && pending.operator() == Some(OPERATOR)).then(
        || {
            pending
                .clone()
                .operate(OPERATOR, Target::Motion(Motion::Line))
        },
    )
}

/// Whether `step` starts `gr` or commits it, which
/// [`VimState::feed`](crate::VimState::feed) cancels while the setting is
/// off.
pub(crate) fn takes(step: &Step) -> bool {
    match step {
        Step::Wait(pending) => pending.operator() == Some(OPERATOR),
        Step::Commit(invocation) => {
            invocation.command.as_str() == "vim.operate"
                && serde_json::from_value::<OperateArgs>(invocation.args.clone())
                    .is_ok_and(|args| args.operator == OPERATOR)
        }
        _ => false,
    }
}

/// `vim.operate` with `gr` over a motion or text object: what it covers at
/// the primary caret replaced by the register.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    // `n N * #` as the search they are, and a mark's motion at the mark.
    let measured = match args.target {
        Target::Motion(motion) => {
            let motion = cmdline::resolve(target, motion).map_or(motion, |(resolved, _)| resolved);
            Target::Motion(marks::resolve(target, motion).unwrap_or(motion))
        }
        other => other,
    };
    operator::sync(target.model);
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let head = Cursor::at(&text, target.model.primary_selection().head()).offset();
    let state = &mut *target.state;
    let span = match measured {
        Target::Motion(motion) => {
            let last_find = state.last_find;
            if let Motion::FindForward(_)
            | Motion::FindBackward(_)
            | Motion::TillForward(_)
            | Motion::TillBackward(_) = motion
            {
                state.last_find = Some(motion);
            }
            operator::motion_span(&text, head, motion, args.count, last_find, tab)
        }
        Target::TextObject(object) => {
            text_object::select(&text, head, object, args.count.unwrap_or(1), tab)
        }
        Target::Selection | Target::SelectedLines | Target::SelectedToEnd => Span::Failed,
    };
    // What `` g`[vg`] `` (or `V`) selects: `'[` and `']` as `g@` sets them,
    // an exclusive end left out.
    let over = match span {
        Span::Failed => {
            state.failed = true;
            return Ok(Dispatched::new(Effect::Nothing));
        }
        // A failed `b` still moves the caret, as for any operator.
        Span::Move(offset) => {
            state.failed = true;
            let before = target.model.selections();
            let caret = Cursor::at(&text, offset).offset();
            target.model.set_selections([operator::caret(caret)]);
            state.goals = None;
            return Ok(Dispatched::new(match target.model.selections() != before {
                true => Effect::Changed,
                false => Effect::Nothing,
            }));
        }
        Span::Lines { rows, .. } => {
            let last = *rows.end();
            let end = operator::row_end(&text, last);
            let cursor = Cursor::at(&text, end.saturating_sub(1).max(text.row_start(last)));
            Ok((
                VisualKind::Line,
                text.row_start(*rows.start()),
                cursor.offset(),
            ))
        }
        // At the text's start `'[` can't be after `']`: the grapheme there
        // is taken, as nvim's `decl` stays put.
        Span::Chars(range) if range.is_empty() && range.start == 0 && !text.is_empty() => {
            Ok((VisualKind::Char, 0, 0))
        }
        // `g@` leaves the caret at `'[`.
        Span::Chars(range) if range.is_empty() => Err(range.start),
        Span::Chars(range) => Ok((VisualKind::Char, range.start, range.end - 1)),
    };
    let dispatched = match over {
        Ok(over) => visual::replace(
            target,
            actor,
            visual::Over::Span(over),
            args.register,
            |value, kind, rows| corrected(value, kind, rows, tab),
        )?,
        Err(at) => before_caret(target, actor, args.register, at)?,
    };
    target.state.set_last_change(Change::Operate(args).into());
    Ok(dispatched)
}

/// `"xP` at `at` with the register's text corrected as over characters: an
/// empty span's replacement.
fn before_caret(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    register: Option<char>,
    at: usize,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    if register == Some('/') && registers.last_search().is_none() {
        return Ok(state.fail(crate::messages::no_previous_regular_expression()));
    }
    registers.adopt_inserted(state);
    let value = registers
        .read(register, &mut **host)
        .map_err(|error| CommandError::new(error.to_string()))?;
    let tab = model.tab_size().max(1);
    let value = value
        .map(|value| corrected(value, VisualKind::Char, 1, tab))
        .transpose()?;
    model.set_selections([operator::caret(at)]);
    let args = PutArgs {
        before: true,
        cursor_after: false,
        indent: false,
        count: None,
        register,
    };
    operator::put_value(model, state, actor, args, value)
}

/// `{Visual}gr`: the selection replaced by the register; from `.`, over a
/// block of these display columns (its left one and how many, `None` to
/// each row's end) if given.
pub(crate) fn selection(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    columns: Option<(usize, Option<usize>)>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let tab = target.model.tab_size().max(1);
    let over = visual::Over::Selection(columns);
    let dispatched = visual::replace(target, actor, over, args.register, |value, kind, rows| {
        corrected(value, kind, rows, tab)
    })?;
    target.state.set_last_change(Change::Operate(args).into());
    Ok(dispatched)
}

/// The plugin's `s:CorrectForRegtype`: `value` as it goes in over a span
/// of `kind` and `rows` rows. A block it would make larger than a put may
/// insert is refused before it's built.
fn corrected(
    value: Register,
    kind: VisualKind,
    rows: u32,
    tab: usize,
) -> Result<Register, CommandError> {
    if value.block_width.is_some() {
        return Ok(value);
    }
    // The text as a put reads it.
    let text = value.text.replace("\r\n", "\n").replace('\r', "\n");
    match kind {
        VisualKind::Block => {
            let lines = split(&text);
            match (value.linewise, lines.len()) {
                // One row (or characters), repeated on each of the block's,
                // as wide as its widest.
                (false, _) | (true, 1) if rows > 1 => {
                    let size = lines
                        .iter()
                        .try_fold(0usize, |size, line| size.checked_add(line.len() + 1))
                        .and_then(|size| size.checked_mul(rows as usize));
                    if size.is_none_or(|size| size > operator::MAX_PUT) {
                        return Err(CommandError::new(tr!(
                            "vim-replace-register-too-large",
                            bytes = text.len(),
                            rows = rows,
                            limit = operator::MAX_PUT
                        )));
                    }
                    let width = lines
                        .iter()
                        .map(|line| visual::width_from(line, 0, tab))
                        .max()
                        .unwrap_or(0);
                    let repeated: Vec<String> = (0..rows)
                        .flat_map(|_| lines.iter().map(|line| line.to_string()))
                        .collect();
                    Ok(Register::block(&repeated, width))
                }
                // Several lines become a block as `setreg(x, '', "a\<C-v>")`
                // leaves them: the register's rows and an empty one after
                // them, 0 columns wide, so no copy is padded.
                (true, 2..) => {
                    let body = text.strip_suffix('\n').unwrap_or(&text);
                    let mut rows: Vec<String> = body.split('\n').map(str::to_string).collect();
                    rows.push(String::new());
                    Ok(Register::block(&rows, 0))
                }
                _ => Ok(value),
            }
        }
        _ if value.linewise => {
            let text = text.strip_suffix('\n').unwrap_or(&text);
            Ok(Register::new(text, false))
        }
        _ => Ok(value),
    }
}

/// Vim's `split(text, "\n")`: the lines, an empty first or last one left
/// out.
fn split(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.first() == Some(&"") {
        lines.remove(0);
    }
    lines
}
