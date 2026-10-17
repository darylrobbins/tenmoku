//! `:[line]pu[t][!] [x]` (`:help :put`): register `x` (`"` by default),
//! as [`Registers::read`] gives it, put in as whole lines below the range's
//! last line (the cursor's with no range), above it with `!`, or at the
//! top for line 0. A charwise register goes in as a line, a block's rows
//! are lines. The caret goes to the last new line's first non-blank. The
//! put is one transaction, writes no register and isn't what `.` repeats.
//! `:pu =` (the expression register) is refused: fe-vim has no expressions.

use fe_command::{Actor, Dispatched, Effect};
use fe_editor::commands::origin;

use super::super::{Address, ExError, Handled, first_non_blank, uncommented};
use super::{Check, Entry};
use crate::{
    VimTarget, insert,
    motion::{self, Cursor},
    operator,
};

pub(super) const COMMAND: Entry = Entry {
    name: "put",
    min: 2,
    edits: true,
    parse,
    run,
};

/// The arguments `run` is given: `!` if there was one, then the register's
/// name, if one was given.
fn parse(after: &str, check: &mut Check<'_>) -> Result<String, ExError> {
    // Only the range's last line matters.
    check(false)?;
    let (above, rest) = match after.strip_prefix('!') {
        Some(rest) => ("!", rest),
        None => ("", after),
    };
    let rest = rest.trim_start_matches([' ', '\t']);
    let mut chars = rest.chars();
    let name = chars.next().filter(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '"' | '-' | '_' | '+' | '*' | '/' | '=')
    });
    let name = match name {
        Some('=') => return Err(ExError::Pattern("the = register: not supported".into())),
        Some(name) => name,
        None => {
            return match uncommented(rest).trim_start_matches([' ', '\t']) {
                "" => Ok(above.to_string()),
                rest => Err(ExError::Trailing(rest.to_string())),
            };
        }
    };
    match uncommented(chars.as_str()).trim_start_matches([' ', '\t']) {
        "" => Ok(format!("{above}{name}")),
        rest => Err(ExError::Trailing(rest.to_string())),
    }
}

fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    lines: Option<(i64, i64)>,
    _range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    let (above, name) = match args.strip_prefix('!') {
        Some(name) => (true, name),
        None => (false, args),
    };
    let name = name.chars().next();
    target.model.sync_display();
    let text = target.model.text().clone();
    let last = i64::from(text.row_count());
    let line = match lines {
        Some((_, end)) => end,
        None => {
            let head = target.model.primary_selection().head();
            i64::from(text.offset_to_point(head).row) + 1
        }
    };
    if line > last {
        return Err(ExError::InvalidRange);
    }
    let to = u32::try_from(line.max(0)).unwrap_or(u32::MAX);

    if name == Some('/') && target.registers.last_search().is_none() {
        return Err(ExError::NoPreviousSearch);
    }
    let value = target
        .registers
        .read(name, &mut *target.host)
        .map_err(|error| ExError::Pattern(error.to_string()))?;
    let Some(value) = value.filter(|value| !value.text.is_empty() || value.block_width.is_some())
    else {
        let shown = name.unwrap_or('"');
        return Err(ExError::Pattern(format!(
            "E353: Nothing in register {shown}"
        )));
    };
    let payload = value.text.replace("\r\n", "\n").replace('\r', "\n");
    let payload = payload.strip_suffix('\n').unwrap_or(&payload);
    let new_lines = u32::try_from(payload.split('\n').count()).unwrap_or(u32::MAX);

    let goal = goal(target);

    // The row the new lines start on, and the edit making them.
    let (first, edit) = match (to, above) {
        (0, _) | (_, true) => {
            let row = to.saturating_sub(1);
            let at = text.row_start(row);
            target.state.marks.opening(target.model.snapshot(), row);
            (row, (at..at, format!("{payload}\n")))
        }
        (to, false) => {
            let at = operator::row_end(&text, to - 1);
            (to, (at..at, format!("\n{payload}")))
        }
    };
    insert::adopt_closing(target.state);
    let (transaction, snapshot) = operator::apply(target.model, [edit], origin(actor));
    let after = snapshot.text();
    // Both `u` and `<C-r>` put the caret on the line the put went below (the
    // first for line 0, the line before it for `!`), at its goal column.
    if let Some(id) = transaction {
        let near = to.max(1) - 1;
        let tab = target.model.tab_size().max(1);
        let undone = at_goal(&text, near, goal, tab);
        target.state.carets.record(id, vec![undone]);
        target
            .state
            .carets
            .record_redo(id, vec![at_goal(after, near, goal, tab)]);
    }
    let row = first + new_lines - 1;
    operator::land(target.model, &snapshot, [first_non_blank(after, row)]);
    target.state.goals = None;
    Ok(Ok(
        Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
    ))
}

/// The primary caret's goal column: the one it keeps, or its own.
fn goal(target: &VimTarget<'_>) -> usize {
    let model = &target.model;
    let head = model.primary_selection().head();
    let goals = motion::valid_goals(
        target.state,
        model.snapshot().version(),
        &model.selections(),
    );
    let kept = goals.and_then(|(_, goals)| goals.iter().find(|(at, _)| *at == head)?.1);
    kept.unwrap_or_else(|| {
        let cursor = Cursor::at(model.text(), head);
        cursor.line.goal_column(cursor.ix, model.tab_size().max(1))
    })
}

/// The place at display column `goal` on `row` of `text`, as a row and a
/// byte column.
fn at_goal(text: &fe_text::Text, row: u32, goal: usize, tab: usize) -> (u32, usize) {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.vertical(row, Some(goal), tab);
    (row, cursor.offset() - text.row_start(row))
}
