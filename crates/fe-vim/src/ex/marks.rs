//! `:marks [chars]`: the marks set, as a [`Pick`] for the host to show,
//! with nvim's columns (`mark line  col file/text`) and order: `'`, `a` to
//! `z`, `A` to `Z`, then `[`, `]`, `^`, `.`, `<` and `>` (fe-vim has no `0`
//! to `9` or `"` marks). With characters, only the marks they name, in the
//! same order; none of them set is `E283`. Choosing a row jumps to the mark
//! as `` `x `` does. The name takes no abbreviation: `:mark` is another
//! command.

use fe_command::{Actor, Dispatched, Effect};

use super::super::pick::printable;
use super::super::{Address, ExError, Handled, HostCommand, Pick, PickRow, PickThen, uncommented};
use super::{Check, Entry};
use crate::VimTarget;

pub(super) const COMMAND: Entry = Entry {
    name: "marks",
    min: 5,
    edits: false,
    parse,
    run,
};

/// The mark names, in nvim's order.
const ORDER: &str = "'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ[]^.<>";

fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    Ok(uncommented(after).trim_matches([' ', '\t']).to_string())
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
    target.model.sync_display();
    let snapshot = target.model.snapshot();
    let text = snapshot.text();
    let mut rows = Vec::new();
    for name in ORDER.chars() {
        if !args.is_empty() && !args.contains(name) {
            continue;
        }
        let Some((row, column)) = target.state.mark(name).and_then(|mark| mark.get(snapshot))
        else {
            continue;
        };
        if row >= text.row_count() {
            continue;
        }
        let start = text.row_start(row);
        let line = text.slice_to_string(start..start + text.row_len(row) as usize);
        let line = printable(line.trim_start_matches([' ', '\t']));
        rows.push(PickRow {
            cells: vec![
                name.to_string(),
                (row + 1).to_string(),
                column.to_string(),
                line,
            ],
            then: PickThen::Jump(name),
        });
    }
    if rows.is_empty() && !args.is_empty() {
        let message = format!("E283: No marks matching \"{args}\"");
        return Ok(Ok(target.state.fail(message)));
    }
    target.state.host_command = Some(HostCommand::Pick(Pick {
        title: "Marks",
        header: "mark line  col file/text",
        rows,
    }));
    Ok(Ok(Dispatched::new(Effect::Notify)))
}
