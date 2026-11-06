//! `:ea[rlier]` and `:lat[er]` (`:help :earlier`, `:help :later`): back
//! (or forward) through the undo history by changes, by time or by file
//! write. fe-buffer's history is linear, not nvim's tree: a new change after
//! an undo discards what was undone, so these step along the one line of
//! states `u` and `<C-r>` walk, and can't reach an abandoned branch as nvim
//! can. `g-` and `g+` are the same as `:earlier` and `:later` with a count
//! (see `g_operators.rs`).
//!
//! - `:earlier {N}` and `:later {N}` (`N` 1 when left out) are `N` times
//!   `u` or `<C-r>`, through the same code, so the carets and the marks are
//!   theirs.
//! - `{N}s`, `{N}m`, `{N}h` and `{N}d` go back (forward) to the state as of
//!   `N` seconds, minutes, hours or days before (after) the current one's
//!   time: the time its newest change was made at, or, with every change
//!   undone, the oldest change's. Back, that undoes the changes made after
//!   then; forward, it redoes those made up to then.
//! - `{N}f` goes back (forward) to the text as of the `N`th file write
//!   before (after) the current one, in the order the writes were made, as
//!   nvim counts them: from the write `:earlier` or `:later` last went to,
//!   if nothing has changed, been undone or redone, or been saved since; or
//!   else from the newest write of the current state; or else, with changes
//!   since, from the newest write of a state before it, those changes
//!   counting as one write going back. So
//!   `:earlier 1f` with changes since the last write reverts them, and
//!   without, it goes to the write before, even when that saved a state
//!   later in the history (a save after an undo). Past the oldest write
//!   `:earlier` goes to before the first change, and past the newest
//!   `:later` to the newest state. `0f` goes to the write counted from, and
//!   with none stays. A write whose text undo and redo can no longer reach
//!   is skipped (see `fe_buffer::Buffer::history_writes`): one discarded as
//!   a redo by a newer change, a transaction that grew after it was saved,
//!   or history the limit dropped. nvim keeps a tree, and where two writes
//!   saved one state, it has the newer only, so it can step otherwise there.
//!
//! Going past the oldest or newest state stops there and doesn't fail, as
//! in nvim (which says "Already at oldest change"). Any other argument is
//! `E475: Invalid argument`; neither takes a range or a `!`. Neither starts
//! a transaction of its own, and `.` doesn't repeat them.

use std::collections::HashMap;

use fe_buffer::{Buffer, HistoryTime, Origin, TransactionId};
use fe_command::{Actor, Dispatched, Effect};
use fe_editor::commands::origin;

use super::super::{Address, ExError, Handled, uncommented};
use super::{Check, Entry};
use crate::{
    VimTarget,
    history::{self, LandedWrite},
};

pub(super) const EARLIER: Entry = Entry {
    name: "earlier",
    min: 2,
    edits: true,
    parse,
    run: run_earlier,
};

pub(super) const LATER: Entry = Entry {
    name: "later",
    min: 3,
    edits: true,
    parse,
    run: run_later,
};

/// How far `:earlier` or `:later` goes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Step {
    Changes(u64),
    Millis(u64),
    Writes(u64),
}

/// `arg`, trimmed, read as a [`Step`]: digits and at most one unit after
/// them, nothing between.
fn read(arg: &str) -> Option<Step> {
    if arg.is_empty() {
        return Some(Step::Changes(1));
    }
    let digits = arg.len() - arg.trim_start_matches(|ch: char| ch.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let count = arg[..digits].parse::<u64>().unwrap_or(u64::MAX);
    let unit: u64 = match &arg[digits..] {
        "" => return Some(Step::Changes(count)),
        "f" => return Some(Step::Writes(count)),
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    Some(Step::Millis(count.saturating_mul(unit)))
}

/// The argument, trimmed and checked.
fn parse(after: &str, _check: &mut Check<'_>) -> Result<String, ExError> {
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    let arg = uncommented(after).trim_matches([' ', '\t']);
    match read(arg) {
        Some(_) => Ok(arg.to_string()),
        None => Err(ExError::InvalidArgument(arg.to_string())),
    }
}

fn run_earlier(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    travel(target, actor, range, args, false)
}

fn run_later(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    _lines: Option<(i64, i64)>,
    range: &[Address],
    args: &str,
) -> Result<Handled, ExError> {
    travel(target, actor, range, args, true)
}

fn travel(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    range: &[Address],
    args: &str,
    later: bool,
) -> Result<Handled, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    let step = read(args).ok_or_else(|| ExError::InvalidArgument(args.to_string()))?;
    if let Step::Changes(count) = step {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        return Ok(history::step_quietly(target, actor, count, later));
    }
    crate::commands::follow(target);
    crate::insert::adopt_closing(target.state);
    let origin = origin(actor);
    let Some(plan) = plan(target, &origin, step, later) else {
        return Ok(Ok(Dispatched::new(Effect::Nothing)));
    };
    let steps = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
    let stepped = if plan.goal < plan.now {
        history::step_quietly(target, actor, steps(plan.now - plan.goal), false)
    } else {
        history::step_quietly(target, actor, steps(plan.goal - plan.now), true)
    };
    if let Step::Writes(_) = step {
        // Count the next from the write gone to, if it got there.
        let buffer = target.model.buffer();
        let state = here(&buffer.history_times(|of| *of == origin));
        let (revision, last_write) = (buffer.history_revision(), last_write(&buffer, &origin));
        drop(buffer);
        target.state.landed_write =
            plan.write
                .filter(|_| state.0 == plan.goal)
                .map(|number| LandedWrite {
                    number,
                    state,
                    revision,
                    last_write,
                });
    }
    Ok(stepped)
}

/// Where `:earlier` or `:later` goes, in states: how many of the origin's
/// changes are applied (0 with all of them undone, up to every change).
struct Plan {
    now: usize,
    goal: usize,
    /// For a step by write, the write `goal` is the state of (0 before the
    /// first change), or `None` past the newest.
    write: Option<u64>,
}

/// The current state, from the history's times: how many changes are
/// applied, and the newest.
fn here(times: &[HistoryTime]) -> (usize, Option<TransactionId>) {
    let now = times.iter().take_while(|time| !time.undone).count();
    (now, now.checked_sub(1).map(|ix| times[ix].transaction))
}

/// The newest write's number, of those `origin`'s changes can reach; 0 for
/// none.
fn last_write(buffer: &Buffer, origin: &Origin) -> u64 {
    (buffer.history_writes(|of| of == origin).last()).map_or(0, |write| write.number)
}

/// Where a step by time or by write goes, for `origin`'s changes; `None`
/// when there's nowhere to go.
fn plan(target: &VimTarget<'_>, origin: &Origin, step: Step, later: bool) -> Option<Plan> {
    let buffer = target.model.buffer();
    let times = buffer.history_times(|of| of == origin);
    let (now, newest_applied) = here(&times);
    let newest = times.len();
    let goal = match step {
        Step::Changes(_) => return None,
        Step::Millis(millis) => {
            // When state `k` (from 1) was made.
            let at = |k: usize| times[k - 1].at_unix_ms;
            let current = match (now, times.first()) {
                (0, Some(first)) => first.at_unix_ms,
                (0, None) => return None,
                (now, _) => at(now),
            };
            let mut goal = now;
            if later {
                let until = current.saturating_add(millis);
                while goal < newest && at(goal + 1) <= until {
                    goal += 1;
                }
            } else {
                let since = current.checked_sub(millis);
                while goal > 0 && since.is_none_or(|since| at(goal) > since) {
                    goal -= 1;
                }
            }
            goal
        }
        Step::Writes(count) => {
            let states: HashMap<TransactionId, usize> = (times.iter().enumerate())
                .map(|(ix, time)| (time.transaction, ix + 1))
                .collect();
            // Each write's number and state, in the order they were made.
            let writes: Vec<(u64, usize)> = buffer
                .history_writes(|of| of == origin)
                .into_iter()
                .filter_map(|write| {
                    let state = match write.transaction {
                        None => 0,
                        Some(id) => *states.get(&id)?,
                    };
                    Some((write.number, state))
                })
                .collect();
            // Unless nothing else happened since, count from the state.
            let (revision, newest_write) = (buffer.history_revision(), last_write(&buffer, origin));
            let landed = (target.state.landed_write).filter(|landed| {
                landed.state == (now, newest_applied)
                    && landed.revision == revision
                    && landed.last_write == newest_write
            });
            let (position, at_write) = position(&writes, now, landed);
            let (goal, write) = travel_writes(&writes, position, at_write, count, later)?;
            return Some(Plan {
                now,
                goal: goal.unwrap_or(newest),
                write,
            });
        }
    };
    Some(Plan {
        now,
        goal,
        write: None,
    })
}

/// The write the current state counts from, as its place in `writes` from
/// 1 (0 for none), and whether the state is that write's (no changes since
/// it): the one `landed` on if it's still there; or else the newest write of
/// this state, or else the newest of a state before it; or else none.
fn position(writes: &[(u64, usize)], now: usize, landed: Option<LandedWrite>) -> (usize, bool) {
    let landed = landed.and_then(|landed| match landed.number {
        0 => Some(0),
        number => (writes.iter())
            .position(|&(of, _)| of == number)
            .map(|ix| ix + 1),
    });
    if let Some(position) = landed {
        return (position, true);
    }
    if let Some(ix) = writes.iter().rposition(|&(_, state)| state == now) {
        return (ix + 1, true);
    }
    match writes.iter().rposition(|&(_, state)| state < now) {
        Some(ix) => (ix + 1, false),
        None => (0, false),
    }
}

/// `count` writes back (or forward if `later`) from `position`, as nvim
/// counts them: changes since the write count as one going back. The goal
/// state (`None` for the newest) and the write's number (0 before the first
/// change, `None` past the newest); `None` for `0f` with no write to go to.
fn travel_writes(
    writes: &[(u64, usize)],
    position: usize,
    at_write: bool,
    count: u64,
    later: bool,
) -> Option<(Option<usize>, Option<u64>)> {
    let nth = |position: usize| writes[position - 1];
    let ix = u64::try_from(position).unwrap_or(u64::MAX);
    if count == 0 {
        let (number, state) = (position > 0).then(|| nth(position))?;
        return Some((Some(state), Some(number)));
    }
    if later {
        let goal = ix.saturating_add(count);
        return Some(
            match usize::try_from(goal)
                .ok()
                .filter(|&goal| goal <= writes.len())
            {
                Some(goal) => {
                    let (number, state) = nth(goal);
                    (Some(state), Some(number))
                }
                None => (None, None),
            },
        );
    }
    let back = count - u64::from(!at_write);
    Some(match ix.checked_sub(back).filter(|&goal| goal > 0) {
        Some(goal) => {
            let (number, state) = nth(goal as usize);
            (Some(state), Some(number))
        }
        None => (Some(0), Some(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::{Step, read};

    #[test]
    fn arguments_read_as_nvim_reads_them() {
        assert_eq!(read(""), Some(Step::Changes(1)));
        assert_eq!(read("0"), Some(Step::Changes(0)));
        assert_eq!(read("12"), Some(Step::Changes(12)));
        assert_eq!(read("10s"), Some(Step::Millis(10_000)));
        assert_eq!(read("2m"), Some(Step::Millis(120_000)));
        assert_eq!(read("1h"), Some(Step::Millis(3_600_000)));
        assert_eq!(read("1d"), Some(Step::Millis(86_400_000)));
        assert_eq!(read("3f"), Some(Step::Writes(3)));
        assert_eq!(read("99999999999999999999d"), Some(Step::Millis(u64::MAX)));
        for bad in ["s", "f", "x", "1x", "1 s", "1sf", "-1", "+1", "1.5s"] {
            assert_eq!(read(bad), None, "{bad}");
        }
    }
}
