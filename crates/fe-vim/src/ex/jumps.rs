//! `:ju[mps]` and `:changes`: the jumplist and the changelist, as a [`Pick`]
//! for the host to show, with nvim's columns (`jump line  col file/text`,
//! `change line  col text`) and order, oldest first. Each row's first cell is
//! how many entries it is from the one the list is at (`<C-o>` and `<C-i>`,
//! or `g;` and `g,`, last went to), `>` before the one it is at; when it is
//! past the newest entry, a last row of just `>`. The text is the row's
//! content without its indent. Choosing a row goes there as `<C-o>` or
//! `<C-i>` (`g;` or `g,`) with its count would, so the list's position
//! moves. Neither takes a range, a `!` or arguments.

use fe_command::{Actor, Dispatched, Effect};

use super::super::pick::printable;
use super::super::{Address, ExError, Handled, HostCommand, Pick, PickRow, PickThen, uncommented};
use super::{Check, Entry};
use crate::{VimTarget, marks};

pub(super) const JUMPS: Entry = Entry {
    name: "jumps",
    min: 2,
    edits: false,
    parse,
    run: run_jumps,
};

pub(super) const CHANGES: Entry = Entry {
    name: "changes",
    min: 7,
    edits: false,
    parse,
    run: run_changes,
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

fn run_jumps(
    target: &mut VimTarget<'_>,
    _actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    _args: &str,
) -> Result<Handled, ExError> {
    show(target, range, true)
}

fn run_changes(
    target: &mut VimTarget<'_>,
    _actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    _args: &str,
) -> Result<Handled, ExError> {
    show(target, range, false)
}

fn show(target: &mut VimTarget<'_>, range: &[Address], jumps: bool) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    target.model.sync_display();
    target.state.marks.watch(target.model);
    marks::catch_up(target.state);
    let snapshot = target.model.snapshot().clone();
    let (entries, at) = match jumps {
        true => target.state.marks.jump_entries(&snapshot),
        false => target.state.marks.change_entries(&snapshot),
    };
    let text = snapshot.text();
    let head = target.model.primary_selection().head();
    let at_head = text.offset_to_point(head);
    let stay = PickThen::At {
        row: at_head.row,
        column: at_head.column,
    };
    // `<C-o>` from past the newest entry first drops it if it is on the
    // cursor's row ([`Marks::travel`]), so each older entry is one fewer
    // away, and the newest is the cursor's own row.
    let dropped =
        jumps && at == entries.len() && entries.last().is_some_and(|&(row, _)| row == at_head.row);
    let mut rows = Vec::with_capacity(entries.len() + 1);
    for (ix, &(row, column)) in entries.iter().enumerate() {
        let line = match row < text.row_count() {
            true => {
                let start = text.row_start(row);
                let line = text.slice_to_string(start..start + text.row_len(row) as usize);
                printable(line.trim_start_matches([' ', '\t']))
            }
            false => String::new(),
        };
        let count = ix.abs_diff(at) as u32;
        let older = ix < at;
        let newest = dropped && ix + 1 == entries.len();
        let then = match (count, jumps) {
            (0, _) => PickThen::At {
                row,
                column: column as u32,
            },
            _ if newest => PickThen::At {
                row,
                column: column as u32,
            },
            (count, true) if dropped => PickThen::Jumps {
                older,
                count: count - 1,
            },
            (count, true) => PickThen::Jumps { older, count },
            (count, false) => PickThen::Changes { older, count },
        };
        let marker = if ix == at { ">" } else { "" };
        rows.push(PickRow {
            cells: vec![
                format!("{marker}{count}"),
                (row + 1).to_string(),
                column.to_string(),
                line,
            ],
            then,
        });
    }
    if at >= entries.len() {
        let empty = |_| String::new();
        rows.push(PickRow {
            cells: [">".to_string()]
                .into_iter()
                .chain((1..4).map(empty))
                .collect(),
            then: stay,
        });
    }
    let (title, header) = match jumps {
        true => ("Jumps", "jump line  col file/text"),
        false => ("Changes", "change line  col text"),
    };
    target.state.host_command = Some(HostCommand::Pick(Pick {
        title,
        header,
        rows,
    }));
    Ok(Ok(Dispatched::new(Effect::Notify)))
}
