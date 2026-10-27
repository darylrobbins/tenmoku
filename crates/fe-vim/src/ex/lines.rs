//! The ex line commands: `:y[ank]`, `:j[oin]`, `:m[ove]`, `:t` and
//! `:co[py]`, `:>` and `:<`, and `:sor[t]`, parsed after their names and run
//! over the rows of an ex range, each in one transaction. Where the cursor
//! goes, after each and after `u`, is nvim's, as probed.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{BufferSnapshot, TransactionId};
use fe_command::{Actor, Dispatched, Effect};
use fe_editor::commands::origin;
use fe_text::Text;

use super::{Base, ExCommand, ExError, Handled, Parser, register_and_count, trailing, uncommented};
use crate::{
    CountArgs, Register, VimTarget, edit, insert, marks,
    motion::{self, Cursor},
    operator::{self, row_end},
    register::Write,
    search::{self, Deadline, Interrupted, Pattern},
    visual,
};

/// The line command `name` names, given what follows it, or `None` if it's
/// none of them; `check` is `ex::command`'s.
pub(super) fn command(
    name: &str,
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<Option<ExCommand>, ExError> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    if is_sort(name) {
        check(true)?;
        return sort_args(after).map(Some);
    }
    let copy = name == "t" || is("copy", 2);
    if copy || is("move", 1) {
        if after.starts_with('!') {
            return Err(ExError::NoBang);
        }
        check(true)?;
        // As in nvim, what follows the address is ignored.
        let mut parser = Parser { line: after, at: 0 };
        let to = match parser.address() {
            Some(to) if to.base != Base::All => to,
            _ => return Err(ExError::InvalidRange),
        };
        return Ok(Some(match copy {
            true => ExCommand::Copy { to },
            false => ExCommand::Move { to },
        }));
    }
    let yank = is("yank", 1);
    if !yank && !is("join", 1) {
        return Ok(None);
    }
    let after = uncommented(after);
    let (raw, args) = match after.strip_prefix('!') {
        Some(_) if yank => return Err(ExError::NoBang),
        Some(args) => (true, args),
        None => (false, after),
    };
    check(true)?;
    let (register, count, args) = register_and_count(args, yank)?;
    match yank {
        true => trailing(ExCommand::Yank { register, count }, args),
        false => trailing(ExCommand::Join { raw, count }, print_flags(args)),
    }
    .map(Some)
}

/// Whether `rest`, an ex line from its command, is a `:sort`.
pub(super) fn is_sort(rest: &str) -> bool {
    let name = rest.split(|ch: char| !ch.is_ascii_alphabetic()).next();
    name.is_some_and(|name| name.len() >= 3 && "sort".starts_with(name))
}

/// `args` past the print flags `l`, `p` and `#` that `:j`, `:>` and `:<`
/// take after their count (fe-vim has nowhere to print).
fn print_flags(args: &str) -> &str {
    args.trim_start_matches([' ', '\t', 'l', 'p', '#'])
}

/// `:>` or `:<` (each more of them, blanks between allowed, a shift more)
/// and its `[count]`.
pub(super) fn shift(
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    let left = after.starts_with('<');
    let mut args = uncommented(after);
    let mut times = 0u32;
    while let Some(rest) =
        args.trim_start_matches([' ', '\t'])
            .strip_prefix(if left { '<' } else { '>' })
    {
        times = times.saturating_add(1);
        args = rest;
    }
    if args.starts_with('!') {
        return Err(ExError::NoBang);
    }
    check(true)?;
    let (_, count, args) = register_and_count(args, false)?;
    trailing(ExCommand::Shift { left, times, count }, print_flags(args))
}

/// `:sort`'s arguments: `!`, then flags and a pattern in any order.
fn sort_args(after: &str) -> Result<ExCommand, ExError> {
    let (reverse, mut args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    let (mut ignore_case, mut numeric, mut unique, mut by_match) = (false, false, false, false);
    let mut pattern = None;
    loop {
        args = args.trim_start_matches([' ', '\t']);
        let Some(ch) = args.chars().next() else {
            break;
        };
        match ch {
            'i' => ignore_case = true,
            // One number kind, as in nvim.
            'n' if numeric => return Err(ExError::TwoNumberKinds),
            'n' => numeric = true,
            'u' => unique = true,
            'r' => by_match = true,
            '"' => break,
            // Hexadecimal, octal, binary, float and locale orders.
            'x' | 'o' | 'b' | 'f' | 'l' => {
                return Err(ExError::Unsupported(format!(":sort {ch}")));
            }
            _ if ch.is_ascii_alphabetic() => {
                return Err(ExError::InvalidArgument(args.to_string()));
            }
            // Anything else delimits a pattern: one, as in nvim.
            _ if pattern.is_some() => return Err(ExError::InvalidArgument(args.to_string())),
            delimiter => {
                let rest = &args[delimiter.len_utf8()..];
                let (found, after) = search::split_search(rest, delimiter);
                args = after.ok_or_else(|| ExError::MissingDelimiter(rest.to_string()))?;
                pattern = Some(found);
                continue;
            }
        }
        args = &args[1..];
    }
    Ok(ExCommand::Sort {
        reverse,
        ignore_case,
        numeric,
        unique,
        by_match,
        pattern,
    })
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
fn at_goal(text: &Text, row: u32, goal: usize, tab: usize) -> (u32, usize) {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.vertical(row, Some(goal), tab);
    (row, cursor.offset() - text.row_start(row))
}

/// The first non-blank on `row`, as an offset.
fn first_non_blank(text: &Text, row: u32) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.go_to_first_non_blank();
    cursor.offset()
}

/// Applies `edits` as one transaction for `actor`, returning it and the
/// snapshot right after. With no edits (the command left the text as it
/// was), it's still an undo step, as nvim's is: a character put in at the
/// text's end and taken out again, which no mark or caret sees.
fn apply(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    edits: Vec<(Range<usize>, String)>,
) -> (Option<TransactionId>, BufferSnapshot) {
    insert::adopt_closing(target.state);
    if !edits.is_empty() {
        return operator::apply(target.model, edits, origin(actor));
    }
    let end = target.model.text().len();
    let steps = [
        vec![(end..end, " ".to_string())],
        vec![(end..end + 1, String::new())],
    ];
    operator::apply_steps(target.model, steps, origin(actor))
}

/// Notes `undone` (a row and byte column in the text before
/// `transaction`) as where `u` puts the caret, and `redone` (in the text
/// after it), if given, as where `<C-r>` does; returns the transaction.
fn noted(
    target: &mut VimTarget<'_>,
    transaction: Option<TransactionId>,
    undone: (u32, usize),
    redone: Option<(u32, usize)>,
) -> Vec<TransactionId> {
    if let Some(id) = transaction {
        target.state.carets.record(id, vec![undone]);
        if let Some(redone) = redone {
            target.state.carets.record_redo(id, vec![redone]);
        }
    }
    transaction.into_iter().collect()
}

/// Where nvim's `<C-r>` puts the caret after `:m` or `:t`: back where it
/// was (`saved`, a row and byte column) if that's `near` what the redo
/// changes, else on `row` of `after`, at the display column `u` left it at
/// (`undone`, in `before`).
fn redone(
    before: &Text,
    after: &Text,
    (saved, undone): ((u32, usize), (u32, usize)),
    near: bool,
    row: u32,
    tab: usize,
) -> (u32, usize) {
    if near {
        return saved;
    }
    let cursor = Cursor::at(before, before.row_start(undone.0) + undone.1);
    at_goal(after, row, cursor.line.goal_column(cursor.ix, tab), tab)
}

/// One caret at `head` of `snapshot`, keeping `goal` as its goal column if
/// given (else it's reset).
fn land(target: &mut VimTarget<'_>, snapshot: &BufferSnapshot, head: usize, goal: Option<usize>) {
    let VimTarget { model, state, .. } = target;
    operator::land(model, snapshot, [head]);
    state.goals = goal.map(|goal| {
        let head = model.primary_selection().head();
        (model.snapshot().version(), vec![(head, Some(goal))])
    });
}

/// The rows' text, each followed by a line break.
fn rows_text(text: &Text, rows: &RangeInclusive<u32>) -> String {
    let mut lines = text.slice_to_string(text.row_start(*rows.start())..row_end(text, *rows.end()));
    lines.push('\n');
    lines
}

/// `:y`: `rows` yanked linewise into `register`, the cursor staying (its
/// goal column reset if the range `searched`).
pub(super) fn yank(
    target: &mut VimTarget<'_>,
    rows: RangeInclusive<u32>,
    register: Option<char>,
    searched: bool,
) -> Handled {
    let lines = rows_text(target.model.text(), &rows);
    let VimTarget {
        host,
        state,
        registers,
        ..
    } = target;
    if searched {
        state.goals = None;
    }
    let yanked = Register::new(lines, true);
    Ok(
        match registers.write(register, Write::Yank, yanked, &mut **host) {
            Ok(()) => Dispatched::new(Effect::Notify),
            Err(error) => state.fail(error.to_string()),
        },
    )
}

/// `:j`: `rows` joined, as `J` joins them or (`raw`) as they are, the caret
/// on the joined line's first non-blank. With one row, nothing is joined
/// and the caret goes to it, keeping its byte column and goal column. `u`
/// puts it back on the first row at its byte column, and `<C-r>` on the
/// joined row at that byte column, as nvim keeps it (not cut to the first
/// row's length).
pub(super) fn join(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    raw: bool,
) -> Handled {
    insert::adopt_closing(target.state);
    let (first, last) = (*rows.start(), *rows.end());
    let text = target.model.text().clone();
    let head = target.model.primary_selection().head();
    let column = head - text.row_start(text.offset_to_point(head).row);
    let start = text.row_start(first);
    let at = Cursor::at(&text, start + column.min(text.row_len(first) as usize)).offset();
    if first == last {
        let goal = goal(target);
        let snapshot = target.model.snapshot().clone();
        land(target, &snapshot, at, Some(goal));
        return Ok(Dispatched::new(Effect::Changed));
    }
    target.model.set_selections([operator::caret(at)]);
    let (transactions, snapshot) = match raw {
        true => {
            let edits = (first + 1..=last)
                .map(|row| (row_end(&text, row - 1)..text.row_start(row), String::new()))
                .collect();
            let marks = visual::joined_marks(
                target.state,
                target.model.snapshot(),
                &[(first, last - first + 1)],
                true,
            );
            let (transaction, snapshot) = apply(target, actor, edits);
            visual::move_marks(target.state, marks);
            (
                noted(
                    target,
                    transaction,
                    (first, at - start),
                    Some((first, column)),
                ),
                snapshot,
            )
        }
        false => {
            // `J`'s join, which `.` doesn't repeat. It leaves the caret on
            // the joined row, wherever catching up moved that.
            let last_change = (target.state.last_change.clone(), target.state.changes);
            let count = Some(last - first + 1);
            let joined = edit::join(target, actor, CountArgs { count });
            (target.state.last_change, target.state.changes) = last_change;
            let transactions = joined?.transactions;
            if let Some(&id) = transactions.last() {
                target.state.carets.record_redo(id, vec![(first, column)]);
            }
            (transactions, target.model.snapshot().clone())
        }
    };
    let row = match raw {
        true => first,
        false => {
            snapshot
                .text()
                .offset_to_point(target.model.primary_selection().head())
                .row
        }
    };
    let caret = first_non_blank(snapshot.text(), row);
    land(target, &snapshot, caret, None);
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// `:m`: `rows` moved below line `to` (from 0, outside them), as each row
/// between rewritten in place, so marks stay on their rows. The caret goes
/// to the last row moved, keeping its goal column, unless the line's
/// addresses `searched`: then the goal is where it lands.
pub(super) fn move_rows(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    to: u32,
    searched: bool,
) -> Handled {
    let (first, last) = (*rows.start(), *rows.end());
    let text = target.model.text().clone();
    let goal = goal(target);
    let head = target.model.primary_selection().head();
    let cursor = text.offset_to_point(head).row;
    // Lines from 1 (`to` is one): `to` is the line above where they go.
    let (f, e, d, c) = (first + 1, last + 1, to, cursor + 1);
    let n = e - f + 1;
    let line_count = text.row_count();
    // Already there: only the caret moves, to the last of them.
    if d == e || d + 1 == f {
        let snapshot = target.model.snapshot().clone();
        land_at_goal(target, &snapshot, last, goal, searched);
        return Ok(Dispatched::new(Effect::Changed));
    }
    // The rows from the range to the destination, turned round, and where
    // the last one moved goes.
    let (top, bottom) = match d > e {
        true => (first, d - 1),
        false => (d, last),
    };
    let mut reordered: Vec<String> = (top..=bottom).map(|row| text.row_to_string(row)).collect();
    let moved_to = match d > e {
        true => {
            reordered.rotate_left(n as usize);
            d - 1
        }
        false => {
            reordered.rotate_right(n as usize);
            d + n - 1
        }
    };
    let (transaction, snapshot) = apply(target, actor, rewrite(&text, top, &reordered));
    let landed = land_at_goal(target, &snapshot, moved_to, goal, searched);
    // Where `u` puts the caret, as nvim's does: back on its line (at its
    // goal) if near what changed, else (at the goal the move left) on the
    // first line moved (moving down) or the line after them (moving up).
    let near = |from: u32, to: u32| (from..=to).contains(&c);
    let back = match d > e {
        true if near(f - 1, e + 1) || near(d, d + 1) => None,
        true => Some(f),
        false if near(d, d + 1) || near(e - u32::from(e == line_count), e + n + 1) => None,
        false => Some((e + 1).min(line_count)),
    };
    let tab = target.model.tab_size().max(1);
    let undone = match back {
        None => at_goal(&text, cursor, goal, tab),
        Some(line) => at_goal(&text, line - 1, landed, tab),
    };
    // And `<C-r>`: back where it was if near either end of the move (the
    // line before a move down's last too, at the text's end), else on the
    // line after the destination, at the column `u` left.
    let near_redo = match d > e {
        true => near(f - 1, f) || near(d, d + n + 1) || d == line_count && c + 1 == d,
        false => near(d, d + n + 1) || near(f + n - 1, f + n),
    };
    let saved = (cursor, head - text.row_start(cursor));
    let line = (d + 1).min(line_count);
    let redone = redone(
        &text,
        snapshot.text(),
        (saved, undone),
        near_redo,
        line - 1,
        tab,
    );
    let transactions = noted(target, transaction, undone, Some(redone));
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// One caret on `row` of `snapshot` at display column `goal`, which it
/// keeps, or if `reset`, the goal is where it lands; returns the goal.
fn land_at_goal(
    target: &mut VimTarget<'_>,
    snapshot: &BufferSnapshot,
    row: u32,
    goal: usize,
    reset: bool,
) -> usize {
    let text = snapshot.text();
    let (row, column) = at_goal(text, row, goal, target.model.tab_size().max(1));
    let head = text.row_start(row) + column;
    let landed = match reset {
        true => {
            let cursor = Cursor::at(text, head);
            cursor
                .line
                .goal_column(cursor.ix, target.model.tab_size().max(1))
        }
        false => goal,
    };
    land(target, snapshot, head, (!reset).then_some(goal));
    landed
}

/// Edits making the rows from `top` hold `lines`, each row whose text
/// changes rewritten in place.
fn rewrite(text: &Text, top: u32, lines: &[String]) -> Vec<(Range<usize>, String)> {
    (top..)
        .zip(lines)
        .filter_map(|(row, line)| {
            let start = text.row_start(row);
            let range = start..row_end(text, row);
            (text.slice_to_string(range.clone()) != *line).then(|| (range, line.clone()))
        })
        .collect()
}

/// `:t`: `rows` copied below line `to` (from 0). The caret goes to the
/// last copy, keeping its goal column, unless the line's addresses
/// `searched`: then the goal is where it lands.
pub(super) fn copy(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    to: u32,
    searched: bool,
) -> Handled {
    let text = target.model.text().clone();
    let goal = goal(target);
    let head = target.model.primary_selection().head();
    let cursor = text.offset_to_point(head).row;
    let lines = rows_text(&text, &rows);
    let edit = match to {
        0 => {
            // Rows go in above the first: its marks move down with it.
            target.state.marks.opening(target.model.snapshot(), 0);
            (0..0, lines)
        }
        to => {
            let at = row_end(&text, to - 1);
            (at..at, format!("\n{}", &lines[..lines.len() - 1]))
        }
    };
    let (transaction, snapshot) = apply(target, actor, vec![edit]);
    let copied = rows.end() - rows.start() + 1;
    let landed = land_at_goal(target, &snapshot, to + copied - 1, goal, searched);
    // As nvim's `u`: back on the caret's line (at its goal) if next to where
    // the copies went (or the line before, below the last), else on the line
    // after, at the goal the copy left.
    let (c, line_count) = (cursor + 1, text.row_count());
    let near = (to..=to + 1).contains(&c) || to == line_count && c + 1 == to;
    let tab = target.model.tab_size().max(1);
    let undone = match near {
        true => at_goal(&text, cursor, goal, tab),
        false => at_goal(&text, to.min(line_count - 1), landed, tab),
    };
    // And `<C-r>`: back where it was if within or next to the copies, else
    // on the first copy, at the column `u` left.
    let near_redo = (to..=to + copied + 1).contains(&c);
    let saved = (cursor, head - text.row_start(cursor));
    let redone = redone(&text, snapshot.text(), (saved, undone), near_redo, to, tab);
    let transactions = noted(target, transaction, undone, Some(redone));
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// `:>` or `:<` (`left`): `rows` shifted `times` over, as `>>` and `<<` shift
/// them. A jump, as in nvim. The caret goes to the last row, at the column
/// of the first non-blank on the last row that changed or is empty, or with
/// none, where its goal column puts it on the first row (as nvim leaves it,
/// within the last row); its goal column is left as it was, unless the
/// range `searched`: then it's where it lands. `u` puts it on the first row
/// at the goal column it had.
pub(super) fn shift_rows(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    left: bool,
    times: u32,
    searched: bool,
) -> Handled {
    marks::jump_here(target);
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let goal = goal(target);
    let (unit, tabs) = operator::shift_unit(target.model);
    let by = unit.saturating_mul(times as usize);
    let edits: Vec<_> = rows
        .clone()
        .filter_map(|row| shifted(&text, row, left, (tab, tabs), by))
        .collect();
    let changed: Vec<u32> = edits
        .iter()
        .map(|(range, _)| text.offset_to_point(range.start).row)
        .collect();
    let (transaction, snapshot) = apply(target, actor, edits);
    let (first, last) = (*rows.start(), *rows.end());
    let transactions = noted(target, transaction, at_goal(&text, first, goal, tab), None);
    let after = snapshot.text();
    let column = rows
        .rev()
        .find(|&row| text.row_len(row) == 0 || changed.binary_search(&row).is_ok())
        .map_or(at_goal(&text, first, goal, tab).1, |row| {
            first_non_blank(after, row) - after.row_start(row)
        });
    let caret = after.row_start(last) + column.min(after.row_len(last) as usize);
    let caret = Cursor::at(after, caret).offset();
    land(target, &snapshot, caret, (!searched).then_some(goal));
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// `row`'s leading blanks rewritten as spaces (or tabs and spaces if
/// `tabs`), `by` columns less wide
/// (`left`) or wider; `None` for an empty row, or one whose indent wouldn't
/// change.
fn shifted(
    text: &Text,
    row: u32,
    left: bool,
    (tab, tabs): (usize, bool),
    by: usize,
) -> Option<(Range<usize>, String)> {
    let line = text.row_to_string(row);
    let blanks = line.len() - line.trim_start_matches([' ', '\t']).len();
    if line.is_empty() {
        return None;
    }
    let width = line[..blanks].bytes().fold(0, |column, byte| match byte {
        b'\t' => column + tab - column % tab,
        _ => column + 1,
    });
    let spaces = operator::indent_fill(
        match left {
            true => width.saturating_sub(by),
            false => width.saturating_add(by),
        },
        tab,
        tabs,
    );
    let start = text.row_start(row);
    (spaces != line[..blanks]).then(|| (start..start + blanks, spaces))
}

/// What a row sorts by.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// `n`'s: none sorts before any number.
    Number(Option<i64>),
    Text(Vec<u8>),
}

/// `:sort`: `rows` sorted as `sort` (an [`ExCommand::Sort`]) says, each
/// row whose text changes rewritten in place, so marks stay on their rows;
/// rows `u` drops are deleted from the end. The caret goes to the first
/// row's first non-blank. `u` puts it back where it was if that's within
/// the rows or next to them, else on the first row changed, at the goal
/// column the sort left.
pub(super) fn sort(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: RangeInclusive<u32>,
    sort: &ExCommand,
) -> Result<Handled, ExError> {
    let ExCommand::Sort {
        reverse,
        ignore_case,
        numeric,
        unique,
        by_match,
        pattern,
    } = sort
    else {
        unreachable!("ex::execute runs only :sort here");
    };
    let (first, last) = (*rows.start(), *rows.end());
    // nvim leaves a single row, and the caret, alone.
    if first == last {
        return Ok(Ok(Dispatched::new(Effect::Notify)));
    }
    let pattern = match pattern.as_deref() {
        None => None,
        Some("") => {
            let last = target.registers.last_search();
            Some(last.ok_or(ExError::NoPreviousSearch)?.0.to_string())
        }
        Some(pattern) => Some(pattern.to_string()),
    };
    let pattern = match pattern {
        Some(pattern) => {
            let ignore_case = target.registers.ignores_case(&pattern, false);
            Some(
                Pattern::parse_with(&pattern, ignore_case)
                    .map_err(|error| ExError::Pattern(error.to_string()))?,
            )
        }
        None => None,
    };
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let lines: Vec<String> = rows.clone().map(|row| text.row_to_string(row)).collect();
    // Every key is read before anything changes: past the time limit,
    // nothing does, as for `:s`.
    let deadline = Deadline::new();
    let keys: Vec<Key> = rows
        .clone()
        .zip(&lines)
        .map(|(row, line)| {
            let key = match &pattern {
                None => line.as_str(),
                Some(pattern) => match search::first_in_row(&text, row, pattern, &deadline)? {
                    Some(found) => {
                        let start = text.row_start(row);
                        let found = found.start - start..found.end - start;
                        match by_match {
                            true => &line[found],
                            false => &line[found.end..],
                        }
                    }
                    None => "",
                },
            };
            Ok(match (numeric, ignore_case) {
                (true, _) => Key::Number(number(key)),
                (false, true) => Key::Text(key.bytes().map(|b| b.to_ascii_lowercase()).collect()),
                (false, false) => Key::Text(key.as_bytes().to_vec()),
            })
        })
        .collect::<Result<_, Interrupted>>()
        .map_err(|interrupted| ExError::Pattern(interrupted.to_string()))?;
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    if *reverse {
        order.reverse();
    }
    let mut sorted: Vec<String> = order.into_iter().map(|ix| lines[ix].clone()).collect();
    if *unique {
        sorted.dedup_by(|line, kept| match ignore_case {
            true => line.eq_ignore_ascii_case(kept),
            false => line == kept,
        });
    }
    let cursor = text
        .offset_to_point(target.model.primary_selection().head())
        .row;
    let mut edits = rewrite(&text, first, &sorted);
    let kept = first + sorted.len() as u32;
    // The first row changed: rewritten, or else the first `u` dropped.
    let changed = match edits.first() {
        Some((range, _)) => Some(text.offset_to_point(range.start).row),
        None => (kept <= last).then_some(kept),
    };
    if kept <= last {
        // As nvim's, the marks on the rows dropped and the one before them
        // go.
        let snapshot = target.model.snapshot();
        target.state.marks.deleting(snapshot, kept - 1, last);
        edits.push((
            row_end(&text, kept - 1)..row_end(&text, last),
            String::new(),
        ));
    }
    // Sorted already, it's back on the first row.
    let undone = match changed.unwrap_or(first) {
        row if cursor + 1 < first || cursor > last + 1 => {
            let line = motion::Line::of(0, sorted[0].clone());
            let column = line.goal_column(line.first_non_blank(), tab);
            at_goal(&text, row, column, tab)
        }
        _ => at_goal(&text, cursor, goal(target), tab),
    };
    let (transaction, snapshot) = apply(target, actor, edits);
    let transactions = noted(target, transaction, undone, None);
    let caret = first_non_blank(snapshot.text(), first);
    land(target, &snapshot, caret, None);
    Ok(Ok(
        Dispatched::new(Effect::Changed).with_transactions(transactions)
    ))
}

/// `:sort n`'s number in `key`: its first decimal digits, negative after a
/// `-`, as many as 64 bits hold; with no digit, 0 if it ends in a `-` (as
/// nvim reads it), else none.
fn number(key: &str) -> Option<i64> {
    let digits = key
        .find(|ch: char| ch.is_ascii_digit())
        .unwrap_or(key.len());
    let negative = key[..digits].ends_with('-');
    if digits == key.len() {
        return negative.then_some(0);
    }
    let value = key[digits..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |n, digit| {
            let digit = i64::from(digit - b'0');
            match negative {
                true => n.saturating_mul(10).saturating_sub(digit),
                false => n.saturating_mul(10).saturating_add(digit),
            }
        });
    Some(value)
}
