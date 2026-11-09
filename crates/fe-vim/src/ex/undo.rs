//! `:u[ndo]` and `:red[o]` (`:help :undo`, `:help :redo`): `u` and `<C-r>`
//! through the same code, so the caret, the marks and the failed flag are
//! theirs (running out of changes fails). `:undo 0` undoes until nothing
//! is left, and doesn't fail for having reached the start. `:undo {N}` (or
//! `:{N}undo`) for N above 0 jumps in nvim's undo tree by change number,
//! which fe-vim's linear history doesn't number: it is refused, and nothing
//! changes. `:undo!` is `:undo`; `:redo` takes no range and no `!`. Neither
//! starts a transaction of its own, and `.` doesn't repeat them.

use fe_command::Actor;

use kiln_l10n::tr;

use super::super::{Address, ExError, Handled, uncommented};
use super::{Check, Entry};
use crate::{CountArgs, VimTarget, history};

pub(super) const UNDO: Entry = Entry {
    name: "undo",
    min: 1,
    edits: true,
    parse: parse_undo,
    run: run_undo,
};

pub(super) const REDO: Entry = Entry {
    name: "redo",
    min: 3,
    edits: true,
    parse: parse_redo,
    run: run_redo,
};

/// The change number given as an argument, if any (`:undo 3` gives `"3"`).
fn parse_undo(after: &str, check: &mut Check<'_>) -> Result<String, ExError> {
    // A negative or backwards range is nvim's E16 or E493, not a number.
    check(true)?;
    let rest = after.strip_prefix('!').unwrap_or(after);
    let rest = uncommented(rest).trim_matches([' ', '\t']);
    let digits = rest.len()
        - rest
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len();
    let (number, trailing) = rest.split_at(digits);
    match trailing.trim_start_matches([' ', '\t']) {
        "" => Ok(number.to_string()),
        trailing => Err(ExError::Trailing(trailing.to_string())),
    }
}

fn parse_redo(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    match uncommented(after).trim_matches([' ', '\t']) {
        "" => Ok(String::new()),
        trailing => Err(ExError::Trailing(trailing.to_string())),
    }
}

fn run_undo(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    lines: Option<(i64, i64)>,
    _range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    // A number after the name wins over the range, which is the same number;
    // one too large for a `u64` is a positive one all the same.
    if lines.is_some_and(|(first, end)| first > end) {
        return Err(ExError::Backwards);
    }
    let number = match (args, lines) {
        ("", None) => return step(target, actor, false, 1),
        ("", Some((_, end))) => u64::try_from(end).unwrap_or(1),
        (digits, _) => digits.parse::<u64>().unwrap_or(u64::MAX),
    };
    match number {
        0 => {
            let failed = target.state.failed;
            let done = step(target, actor, false, u32::MAX);
            target.state.failed = failed;
            done
        }
        number => Err(ExError::Pattern(
            tr!("vim-undo-number-not-supported", number = number.to_string()).into(),
        )),
    }
}

fn run_redo(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    _args: &str,
) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    step(target, actor, true, 1)
}

/// `count` steps of `u` (or `<C-r>` if `redo`).
fn step(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    redo: bool,
    count: u32,
) -> Result<Handled, ExError> {
    crate::commands::follow(target);
    Ok(history::undo(
        target,
        actor,
        CountArgs { count: Some(count) },
        redo,
    ))
}
