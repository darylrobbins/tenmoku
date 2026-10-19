//! `:delm[arks] {marks}` and `:delm[arks]!` (`:help :delmarks`): `{marks}`
//! are letters (`a` to `z`, `A` to `Z`), digits (there are no `0` to `9`
//! marks here, so they delete nothing), ranges of one kind (`a-d`), and
//! `^`, `.`, `[`, `]`, `<` and `>`, with blanks between. `'` is not one of
//! them, as in nvim (E475). Marks are deleted left to right, so those
//! before a bad one are gone. With no marks it is E471, a bad one E475 (the
//! message gives what follows from where it was found), and `!` takes no
//! marks (E474): it deletes every lowercase mark, `'^`, `'[`, `']` and
//! `'.`. A range isn't taken. Both are fe-vim's `'<` and `'>` as nvim keeps
//! them: deleting just one makes both read the other end.

use fe_command::{Actor, Dispatched, Effect};

use super::super::{Address, ExError, Handled, uncommented};
use super::{Check, Entry};
use crate::{VimTarget, marks};

pub(super) const COMMAND: Entry = Entry {
    name: "delmarks",
    min: 4,
    edits: false,
    parse,
    run,
};

/// The arguments, with a leading `!` kept when it is `:delmarks!`.
fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    let bang = after.starts_with('!');
    let rest = uncommented(after.strip_prefix('!').unwrap_or(after)).trim_matches([' ', '\t']);
    match (bang, rest) {
        (true, "") => Ok("!".to_string()),
        (true, _) => Err(ExError::TwoNumberKinds),
        (false, "") => Err(ExError::ArgumentRequired),
        (false, rest) => Ok(rest.to_string()),
    }
}

fn run(
    target: &mut VimTarget<'_>,
    _actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    if args == "!" {
        marks::delete_lowercase(target.state);
        return Ok(Ok(Dispatched::new(Effect::Nothing)));
    }
    let chars: Vec<(usize, char)> = args.char_indices().collect();
    let mut at = 0;
    while let Some(&(offset, ch)) = chars.get(at) {
        match kind_of(ch) {
            Some(kind) => {
                let mut last = ch;
                if chars.get(at + 1).is_some_and(|&(_, dash)| dash == '-') {
                    last = match chars.get(at + 2) {
                        Some(&(_, to)) if kind_of(to) == Some(kind) && to >= ch => to,
                        _ => return Err(ExError::InvalidArgument(args[offset..].to_string())),
                    };
                    at += 2;
                }
                for name in ch..=last {
                    marks::delete(target.state, name);
                }
            }
            None if "^.[]<> ".contains(ch) => marks::delete(target.state, ch),
            None => return Err(ExError::InvalidArgument(args[offset..].to_string())),
        }
        at += 1;
    }
    Ok(Ok(Dispatched::new(Effect::Nothing)))
}

fn kind_of(ch: char) -> Option<u8> {
    match ch {
        'a'..='z' => Some(0),
        'A'..='Z' => Some(1),
        '0'..='9' => Some(2),
        _ => None,
    }
}
