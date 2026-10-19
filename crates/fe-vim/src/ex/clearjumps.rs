//! `:cle[arjumps]` (`:help :clearjumps`): empties the jumplist, so `<C-o>`
//! and `<C-i>` have nowhere to go. The jump mark `''` stays. It takes no
//! range, no `!` and no arguments.

use fe_command::{Actor, Dispatched, Effect};

use super::super::{Address, ExError, Handled, uncommented};
use super::{Check, Entry};
use crate::{VimTarget, marks};

pub(super) const COMMAND: Entry = Entry {
    name: "clearjumps",
    min: 3,
    edits: false,
    parse,
    run,
};

fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    match uncommented(after).trim_matches([' ', '\t']) {
        "" => Ok(String::new()),
        trailing => Err(ExError::Trailing(trailing.to_string())),
    }
}

fn run(
    target: &mut VimTarget<'_>,
    _actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    _args: &str,
) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    marks::clear_jumps(target.state);
    Ok(Ok(Dispatched::new(Effect::Nothing)))
}
