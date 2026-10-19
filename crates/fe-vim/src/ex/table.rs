//! The ex commands that are a module of their own, looked up by name in
//! [`COMMANDS`] so that adding one touches neither `ex.rs`, the host's
//! `ex_edits` nor `lib.rs`.
//!
//! To add `:name`:
//!
//! 1. Write `crates/fe-vim/src/ex/<name>.rs` with a `pub(super) const
//!    COMMAND: Entry`: the command's full name and shortest abbreviation
//!    (`"put", 2` for `:pu`), whether it `edits` the buffer (a read-only
//!    buffer refuses it), a pure `parse` of what follows the name, and a
//!    `run` over the line's range. The module documents the command
//!    itself in its module docs, and reaches `ex.rs`'s private helpers
//!    (`counted_lines`, `row`, `to_row_start`, `first_non_blank`,
//!    `register_and_count`, `trailing`, `uncommented`, `Parser`) as
//!    `super::super::…`.
//! 2. Declare it here with `#[path = "<name>.rs"] mod <name>;` and list
//!    `<name>::COMMAND` in [`COMMANDS`].
//!
//! `ex.rs` tries the table after the commands it and `lines.rs` take by
//! name and before it gives up with E492, so a name they take keeps its
//! meaning.

use fe_command::Actor;

use super::{Address, ExError, Handled};

use crate::VimTarget;

#[path = "clearjumps.rs"]
mod clearjumps;
#[path = "delmarks.rs"]
mod delmarks;
#[path = "jumps.rs"]
mod jumps;
#[path = "marks.rs"]
mod marks;
#[path = "put.rs"]
mod put;
#[path = "registers.rs"]
mod registers;
#[path = "set.rs"]
mod set;
#[path = "undo.rs"]
mod undo;

/// One ex command a module defines.
pub(super) struct Entry {
    /// The command's full name, as `:help` gives it.
    pub name: &'static str,
    /// The fewest letters of `name` that name it (`2` for `:pu`).
    pub min: usize,
    /// Whether the command can edit the buffer.
    pub edits: bool,
    /// What follows the name (`after`) read into the arguments `run` is
    /// given, or the error nvim gives. `check` is called once the command
    /// is recognised, before the arguments are read, with whether both of
    /// the range's ends matter to the command.
    pub parse: fn(after: &str, check: &mut Check<'_>) -> Result<String, ExError>,
    /// Runs the command: `lines` is the range resolved (from line 1, `None`
    /// with no range), `range` its addresses, `args` what `parse` read.
    pub run: Run,
}

/// The range check `command()` is given, as `parse` gets it.
pub(super) type Check<'a> = dyn FnMut(bool) -> Result<(), ExError> + 'a;

/// An entry's `run`.
pub(super) type Run = fn(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    lines: Option<(i64, i64)>,
    range: &[Address],
    args: &str,
) -> Result<Handled, ExError>;

impl Entry {
    /// Whether `name` names the command: at least `min` letters of it.
    pub(super) fn is(&self, name: &str) -> bool {
        name.len() >= self.min && self.name.starts_with(name)
    }
}

/// Every command in the table.
pub(super) const COMMANDS: &[Entry] = &[
    clearjumps::COMMAND,
    delmarks::COMMAND,
    jumps::JUMPS,
    jumps::CHANGES,
    marks::COMMAND,
    registers::COMMAND,
    registers::DISPLAY,
    put::COMMAND,
    set::COMMAND,
    undo::UNDO,
    undo::REDO,
];

/// The entry `name` names.
pub(super) fn find(name: &str) -> Option<&'static Entry> {
    COMMANDS.iter().find(|entry| entry.is(name))
}

/// The entry whose full name is `name`.
pub(super) fn named(name: &str) -> Option<&'static Entry> {
    COMMANDS.iter().find(|entry| entry.name == name)
}
