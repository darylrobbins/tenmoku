//! `:reg[isters] [names]` and `:di[splay] [names]`: the registers set, as a
//! [`Pick`] for the host to show, with nvim's columns (`Type Name Content`)
//! and order: `"`, `0` to `9`, `a` to `z`, `-`, `*`, `+`, `.`, `:`, `%`,
//! `#`, `/`. With names, only those, in the same order. An unset register
//! has no row, nor have `*` and `+` when the clipboard holds no text. fe-vim
//! has no file names, so `%` and `#` have none either, and `.` and `:` wait
//! for [`Registers::get`] to give them. Choosing a row puts the register as
//! `"xp` does.

use fe_command::{Actor, Dispatched, Effect};

use super::super::pick::printable;
use super::super::{Address, ExError, Handled, HostCommand, Pick, PickRow, PickThen};
use super::{Check, Entry};
use crate::{Register, VimTarget};

pub(super) const COMMAND: Entry = Entry {
    name: "registers",
    min: 3,
    edits: false,
    parse,
    run,
};

pub(super) const DISPLAY: Entry = Entry {
    name: "display",
    min: 2,
    edits: false,
    parse,
    run,
};

/// The register names, in nvim's order.
const ORDER: &str = "\"0123456789abcdefghijklmnopqrstuvwxyz-*+.:%#/";

/// How many characters of a register's text a row keeps.
const SHOWN: usize = 256;

fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    Ok(after.trim_matches([' ', '\t']).to_string())
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
    let mut rows = Vec::new();
    for name in ORDER.chars() {
        if !args.is_empty() && !args.contains(name) {
            continue;
        }
        let row = if matches!(name, '*' | '+') {
            // Only the clipboard's text is owned, as the host gives it.
            target
                .registers
                .read(Some(name), target.host)
                .ok()
                .flatten()
                .filter(|register| !register.text.is_empty())
                .map(|register| row(name, &register))
        } else {
            target
                .registers
                .get(name)
                .map(|register| row(name, register))
        };
        rows.extend(row);
    }
    target.state.host_command = Some(HostCommand::Pick(Pick {
        title: "Registers",
        header: "Type Name Content",
        rows,
    }));
    Ok(Ok(Dispatched::new(Effect::Notify)))
}

fn row(name: char, register: &Register) -> PickRow {
    PickRow {
        cells: vec![
            kind(register).to_string(),
            format!("\"{name}"),
            content(register),
        ],
        then: PickThen::Put(name),
    }
}

fn kind(register: &Register) -> char {
    if register.block_width.is_some() {
        'b'
    } else if register.linewise {
        'l'
    } else {
        'c'
    }
}

/// The text as nvim lists it: line breaks and control characters shown, a
/// linewise register ending in a line break, cut after [`SHOWN`] characters.
fn content(register: &Register) -> String {
    let mut shown: String = register.text.chars().take(SHOWN).collect();
    let whole = shown.len() == register.text.len();
    if whole && register.linewise && !shown.ends_with('\n') {
        shown.push('\n');
    }
    printable(&shown)
}
