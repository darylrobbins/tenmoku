//! Line and screen motions (a feature module; see [`crate::feature`]):
//! `ge` and `gE` back to the end of a word or WORD, `_ + -` to a row's first
//! non-blank (`_` is [`Motion::Line`]), `|` to a screen column, `H M L` to
//! the view's top, middle and bottom rows, `gj gk` by display rows, `g_` to a
//! row's last non-blank, and `g$ gm` to the end and middle of what the view
//! shows of a display row.
//!
//! `H M L` need the view and `gj gk g$ gm` the display rows and columns, which [`motion`]
//! isn't given: the `vim.move`, `vim.operate`, `vim.repeat` and
//! `vim.select` handlers run inside [`with_view`], which holds them for the
//! command. Outside it, the view is a [`HeadlessHost`]'s with no viewport,
//! `page_rows() + 2` rows from row 0, and `gj gk` move between buffer rows
//! as `j k` do, but charwise.
//!
//! A counted `ge gE gj gk` that fails part way still moves the caret, as
//! nvim's does: under an operator, which then does nothing, through
//! [`moves_when_failing`]; in a visual mode, where it still fails the key,
//! by noting it for [`with_view`].

use std::{cell::RefCell, ops::Range};

use fe_editor::commands::{HeadlessHost, Viewport};
use fe_text::Text;

use crate::{
    CountArgs, Mode, MoveArgs, OperateArgs, Pending, Step, Target, VimKey, VimState, VimTarget,
    args::{Motion, RangeClass},
    feature::Feature,
    insert,
    motion::{Class, Cursor, Line},
    operator,
    repeat::Change,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    ..Feature::NONE
};

/// `_ + - | H M L`, in normal, operator-pending and the visual modes.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let motion = match key {
        VimKey::Char('_') => Motion::Line,
        VimKey::Char('+') => Motion::NextLineFirstNonBlank,
        VimKey::Char('-') => Motion::PrevLineFirstNonBlank,
        VimKey::Char('|') => Motion::Column,
        VimKey::Char('H') => Motion::WindowTop,
        VimKey::Char('M') => Motion::WindowMiddle,
        VimKey::Char('L') => Motion::WindowBottom,
        _ => return None,
    };
    Some(pending.clone().motion(motion))
}

/// `e E j k _ $ m` after `g`.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let motion = match key {
        VimKey::Char('e') => Motion::WordEndBackward,
        VimKey::Char('E') => Motion::BigWordEndBackward,
        VimKey::Char('j') => Motion::DisplayDown,
        VimKey::Char('k') => Motion::DisplayUp,
        VimKey::Char('_') => Motion::LastNonBlank,
        VimKey::Char('$') => Motion::DisplayLineEnd,
        VimKey::Char('m') => Motion::DisplayMiddle,
        _ => return None,
    };
    Some(pending.clone().motion(motion))
}

/// `motion`'s class as an operator's target ([`Motion::class`]).
pub(crate) fn class(motion: Motion) -> RangeClass {
    match motion {
        Motion::WordEndBackward
        | Motion::BigWordEndBackward
        | Motion::LastNonBlank
        | Motion::DisplayLineEnd => RangeClass::Inclusive,
        Motion::NextLineFirstNonBlank
        | Motion::PrevLineFirstNonBlank
        | Motion::WindowTop
        | Motion::WindowMiddle
        | Motion::WindowBottom => RangeClass::Linewise,
        _ => RangeClass::Exclusive,
    }
}

/// Whether `motion` is a jump ([`Motion::is_jump`]): `H M L`.
pub(crate) fn is_jump(motion: Motion) -> bool {
    matches!(
        motion,
        Motion::WindowTop | Motion::WindowMiddle | Motion::WindowBottom
    )
}

/// Whether `motion`, failing having moved an operator's caret, leaves it
/// there (`operator.rs`'s `Span::Move`): `ge gE gj gk`.
pub(crate) fn moves_when_failing(motion: Motion) -> bool {
    matches!(
        motion,
        Motion::WordEndBackward
            | Motion::BigWordEndBackward
            | Motion::DisplayDown
            | Motion::DisplayUp
            | Motion::DisplayLineEnd
    )
}

/// Whether a delete over `motion` writes register `1` even within a row
/// ([`crate::Target::writes_numbered`]): none does.
pub(crate) fn writes_numbered(_motion: Motion) -> bool {
    false
}

/// The command [`with_view`] runs: its motion and count.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Viewed {
    motion: Motion,
    count: Option<u32>,
}

/// What a command's motions see of the view, while [`with_view`] runs.
struct View {
    /// The view's first row and how many rows it has room for.
    top: u32,
    rows: u32,
    /// A visual mode is on.
    visual: bool,
    /// The vim cursor's offset, if it stands past the last character of a
    /// row that has one (after `$`): the motions see it clamped onto it.
    past_end: Option<usize>,
    /// The display columns of an unwrapped row the view shows.
    columns: Range<u32>,
    /// For `gj gk g$ gm`, each caret's display row and the one they go
    /// to, by offset.
    hops: Vec<Hop>,
    /// What `gj` and `gk` left for each caret they moved, for the state's
    /// display goals once the command ran.
    alongs: Vec<DisplayGoal>,
    /// In a visual mode, a motion moved the cursor but failed, which fails
    /// the key (stopping a macro) once the command ran.
    failed: bool,
}

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

/// Puts the view it replaced back when dropped, even in a panic.
struct Restore(Option<View>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        VIEW.with(|view| *view.borrow_mut() = previous);
    }
}

/// Runs `f` with `target`'s view available to [`motion`]: the viewport
/// (`page_rows() + 2` rows from row 0 if the host doesn't know it), whether
/// a visual mode is on and its cursor past a row's end, and if the
/// `viewed` command's motion (with its count) is `gj` or `gk`, the display
/// rows each caret goes from and to. Only those are looked up, never all
/// the rows. Once `f` returns, a visual motion that moved but failed fails
/// the key.
pub(crate) fn with_view<'a, R>(
    target: &mut VimTarget<'a>,
    viewed: Option<Viewed>,
    f: impl FnOnce(&mut VimTarget<'a>) -> R,
) -> R {
    let (top, rows) = match target.host.viewport() {
        Some(Viewport { top_row, rows }) => (top_row, rows),
        None => (0, target.host.page_rows().saturating_add(2)),
    };
    let visual = matches!(target.state.mode, Mode::Visual(_));
    let past_end = visual.then(|| visual_past_end(target)).flatten();
    let version = target.model.snapshot().version();
    let held = target.state.display_goals.clone();
    // Normal mode's goals as `motion::apply` reads them: all or none.
    let goals = crate::motion::valid_goals(target.state, version, &target.model.selections())
        .map(|(_, goals)| goals.clone());
    let columns = target.host.view_columns().unwrap_or(0..80);
    let hops = match viewed {
        Some(Viewed {
            motion: motion @ (Motion::DisplayDown | Motion::DisplayUp),
            count,
            ..
        }) => hops(
            target,
            motion == Motion::DisplayDown,
            count.unwrap_or(1).max(1),
        ),
        // `g$` goes `count - 1` display rows down, `gm` stays on the row.
        Some(Viewed {
            motion: Motion::DisplayLineEnd,
            count,
            ..
        }) => hops(target, true, count.unwrap_or(1).max(1) - 1),
        Some(Viewed {
            motion: Motion::DisplayMiddle,
            ..
        }) => hops(target, true, 0),
        _ => Vec::new(),
    };
    // A display column holds while the buffer goal it came with does.
    let visual_along = target.state.visual_along();
    let hops = hops.into_iter().map(|hop| {
        if visual {
            return Hop {
                along: visual_along,
                ..hop
            };
        }
        let along = held
            .as_ref()
            .zip(goals.as_ref())
            .and_then(|((at, held), goals)| {
                let goal = goals.iter().find(|(head, _)| *head == hop.offset)?.1?;
                let found = held
                    .iter()
                    .find(|d| d.head == hop.offset && d.goal == goal)?;
                (*at == version).then_some(found.along)
            });
        Hop { along, ..hop }
    });
    let hops: Vec<Hop> = hops.collect();
    let view = View {
        top,
        rows,
        visual,
        past_end,
        columns,
        hops,
        alongs: Vec::new(),
        failed: false,
    };
    let restore = Restore(VIEW.with(|cell| cell.replace(Some(view))));
    let result = f(target);
    let failed = VIEW.with(|cell| cell.borrow().as_ref().is_some_and(|view| view.failed));
    drop(restore);
    target.state.failed |= failed;
    result
}

/// The command `vim.move` with `args` runs, for [`with_view`].
pub(crate) fn moved(args: &MoveArgs) -> Option<Viewed> {
    Some(Viewed {
        motion: args.motion,
        count: args.count,
    })
}

/// The command `vim.operate` with `args` runs, for [`with_view`].
pub(crate) fn operated(args: &OperateArgs) -> Option<Viewed> {
    match args.target {
        Target::Motion(motion) => Some(Viewed {
            motion,
            count: args.count,
        }),
        _ => None,
    }
}

/// The command `.` with `args` repeats, for [`with_view`]: the last
/// change's, with the count in place of its own. A session that ended
/// meanwhile (in a clone of the state) is adopted first, as `.` adopts it,
/// so its change is the one looked at.
pub(crate) fn repeated(state: &mut VimState, args: CountArgs) -> Option<Viewed> {
    insert::adopt_closing(state);
    match &state.last_change()?.change {
        Change::Operate(operate) => operated(operate).map(|viewed| Viewed {
            count: args.count.or(viewed.count),
            ..viewed
        }),
        _ => None,
    }
}

/// The vim cursor's offset if it's past the last character of its row.
fn visual_past_end(target: &mut VimTarget<'_>) -> Option<usize> {
    target.model.sync_display();
    let offset = target.state.visual_cursor(target.model)?;
    let text = target.model.text();
    let row = text.offset_to_point(offset).row;
    (text.row_len(row) > 0 && offset == operator::row_end(text, row)).then_some(offset)
}

/// Notes for the [`with_view`] running, if one is, that a visual motion
/// moved but failed.
fn report_failed() {
    VIEW.with(|view| {
        if let Some(view) = view.borrow_mut().as_mut() {
            view.failed = true;
        }
    });
}

/// A caret's step by display rows: from the one holding `offset`, `rows`
/// rows to the one showing `to`.
#[derive(Clone, Debug)]
struct Hop {
    offset: usize,
    from: Range<usize>,
    to: Range<usize>,
    /// `to` ends its buffer row.
    to_last: bool,
    rows: u32,
    /// `to` is a whole buffer row, not a part of a wrapped one.
    to_whole: bool,
    /// The furthest display column along a display row that may be on one:
    /// one less than the most columns the wrap width can be, as how
    /// fe-display broke the display rows near `from` and `to` shows it;
    /// `None` if neither row wraps.
    limit: Option<usize>,
    /// The display column along the row the caret's last `gj` or `gk` aimed
    /// for, if it's still the caret's goal.
    along: Option<usize>,
}

/// A caret's display column along a display row, kept with the buffer
/// column `goal` it came with through `gj` and `gk`: a tab can be wider on
/// a wrapped row than in the buffer, so the buffer column alone can't say
/// where along the row the caret was.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DisplayGoal {
    pub(crate) head: usize,
    pub(crate) goal: usize,
    pub(crate) along: usize,
}

/// The display rows `count` `gj`s (`down`) or `gk`s take each caret
/// between: each selection's head, or in a visual mode the vim cursor.
fn hops(target: &mut VimTarget<'_>, down: bool, count: u32) -> Vec<Hop> {
    target.model.sync_display();
    let offsets = match target.state.visual_cursor(target.model) {
        Some(cursor) => vec![cursor],
        None => target.model.selections().iter().map(|s| s.head()).collect(),
    };
    let tab = target.model.tab_size().max(1);
    let (text, display) = (target.model.text(), target.model.display());
    let last = display.row_count().saturating_sub(1);
    // fe-display breaks a row before the word that doesn't fit, or mid-word
    // if that word is the row's first, with tab stops from each display
    // row's start. So a display row and the start of the next show how wide
    // the wrap is at most: narrower than the row with the next one's first
    // word (or, broken mid-word, its first character). Measured on the
    // display rows within two of `at` in its buffer row.
    let limit = |at: u32| {
        let buffer_row = display.row(at).buffer_row;
        let near: Vec<_> = (at.saturating_sub(2)..=at.saturating_add(3).min(last))
            .map(|row| display.row(row))
            .filter(|row| row.buffer_row == buffer_row)
            .collect();
        if near.len() < 2 {
            return None;
        }
        let line = Line::new(text, buffer_row);
        let ix = |offset: usize| first_grapheme(&line, offset);
        let blank = |ix: usize| line.grapheme(ix).starts_with(char::is_whitespace);
        near.windows(2)
            .map(|pair| {
                let (first, end, next_end) = (
                    ix(pair[0].range.start),
                    ix(pair[0].range.end),
                    ix(pair[1].range.end),
                );
                let columns = columns_between(&line, first, end, tab);
                let word = match end > first && blank(end - 1) {
                    true => (end..next_end).take_while(|&ix| !blank(ix)).count(),
                    false => 1,
                };
                let word: usize = (end..end + word.max(1).min(next_end - end))
                    .map(|ix| line.width(ix, columns, tab))
                    .sum();
                // The wrap is narrower than `columns + word`: one less is
                // the most it holds, and one less again its last column.
                (columns + word).saturating_sub(2)
            })
            .min()
    };
    let mut hops: Vec<Hop> = offsets
        .into_iter()
        .map(|offset| {
            let offset = Cursor::at(text, offset).offset();
            let from = display.row_for_offset(offset);
            let to = match down {
                true => from.saturating_add(count).min(last),
                false => from.saturating_sub(count),
            };
            let (from_row, to_row) = (display.row(from), display.row(to));
            let start = text.row_start(to_row.buffer_row);
            let to_whole =
                to_row.range == (start..start + text.row_len(to_row.buffer_row) as usize);
            let limit = match (limit(from), limit(to)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            Hop {
                offset,
                from: from_row.range,
                to: to_row.range,
                to_last: to_row.last,
                rows: from.abs_diff(to),
                to_whole,
                limit,
                along: None,
            }
        })
        .collect();
    hops.sort_by_key(|hop| hop.offset);
    hops
}

/// [`hops`] by buffer rows, for a caret outside [`with_view`].
fn row_hop(text: &Text, offset: usize, row: u32, down: bool, count: u32) -> Hop {
    let to = match down {
        true => row.saturating_add(count).min(text.row_count() - 1),
        false => row.saturating_sub(count),
    };
    let range = |row: u32| {
        let start = text.row_start(row);
        start..start + text.row_len(row) as usize
    };
    Hop {
        offset,
        from: range(row),
        to: range(to),
        to_last: true,
        rows: row.abs_diff(to),
        to_whole: true,
        limit: None,
        along: None,
    }
}

/// Moves one caret by `args.motion`, as `motion::motion` does: `Ok` holds
/// the goal column it keeps, `Err` the goal to keep when it fails.
pub(crate) fn motion(
    cursor: &mut Cursor,
    args: MoveArgs,
    goal: Option<usize>,
    tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    let count = args.count.unwrap_or(1).max(1);
    let (row, last_row) = (cursor.row, cursor.text.row_count() - 1);
    let (visual, clamped) = VIEW.with(|view| match view.borrow().as_ref() {
        Some(view) => (view.visual, view.past_end),
        None => (false, None),
    });
    match args.motion {
        Motion::WordEndBackward | Motion::BigWordEndBackward => {
            let from = cursor.offset();
            // A visual cursor past the row's end was clamped onto its last
            // character, which is then the first word end back.
            let from_end = visual
                && cursor.ix == cursor.line.last()
                && clamped == Some(cursor.line.start + cursor.line.text.len());
            let big = args.motion == Motion::BigWordEndBackward;
            match word_ends_back(cursor, count, big, from_end) {
                false => Ok(None),
                true => failed(cursor, from, from_end, visual, None),
            }
        }
        // Like `j` and `k`, they fail only where they can't move at all.
        Motion::NextLineFirstNonBlank if row == last_row => Err(goal),
        Motion::PrevLineFirstNonBlank if row == 0 => Err(goal),
        Motion::NextLineFirstNonBlank | Motion::PrevLineFirstNonBlank => {
            cursor.enter(match args.motion {
                Motion::NextLineFirstNonBlank => row.saturating_add(count).min(last_row),
                _ => row.saturating_sub(count),
            });
            cursor.ix = cursor.line.first_non_blank();
            Ok(None)
        }
        // The goal stays the column asked for, even past the row's end.
        Motion::Column => {
            let goal = count as usize - 1;
            cursor.vertical(row, Some(goal), tab);
            past_end(cursor, visual, goal, tab);
            Ok(Some(goal))
        }
        Motion::WindowTop | Motion::WindowMiddle | Motion::WindowBottom => {
            let (top, rows) = VIEW.with(|view| match view.borrow().as_ref() {
                Some(view) => (view.top, view.rows),
                None => (0, HeadlessHost::default().page_rows.saturating_add(2)),
            });
            let top = top.min(last_row);
            // Rows past the text's end hold nothing to go to.
            let shown = rows.min(last_row + 1 - top).max(1);
            let to = match args.motion {
                Motion::WindowTop => top + count.min(shown) - 1,
                Motion::WindowBottom => top + shown - count.min(shown),
                _ => top + (shown - 1) / 2,
            };
            // The column is kept, as `G` keeps it ('nostartofline').
            let goal = cursor.vertical(to, goal, tab);
            if let Some(goal) = goal {
                past_end(cursor, visual, goal, tab);
            }
            Ok(goal)
        }
        // Like `+`, `g_` fails only where it can't move at all.
        Motion::LastNonBlank if count > 1 && row == last_row => Err(goal),
        Motion::LastNonBlank => {
            cursor.enter(row.saturating_add(count - 1).min(last_row));
            let blank = |ix: usize| matches!(cursor.line.grapheme(ix), " " | "\t");
            cursor.ix = (0..cursor.line.len())
                .rev()
                .find(|&ix| !blank(ix))
                .unwrap_or(0);
            Ok(None)
        }
        Motion::DisplayLineEnd | Motion::DisplayMiddle => {
            let offset = cursor.offset();
            let end = args.motion == Motion::DisplayLineEnd;
            let (hop, columns) = VIEW.with(|view| {
                let view = view.borrow();
                let hops = view.as_ref().map_or(&[][..], |view| &view.hops[..]);
                let found = hops.binary_search_by_key(&offset, |hop| hop.offset);
                let columns = view.as_ref().map_or(0..80, |view| view.columns.clone());
                (found.ok().map(|ix| hops[ix].clone()), columns)
            });
            let rows = if end { count - 1 } else { 0 };
            let hop = hop.unwrap_or_else(|| row_hop(cursor.text, offset, row, true, rows));
            let width = columns.end.saturating_sub(columns.start).max(1) as usize;
            // The display column along the row that is the view's right
            // edge (`g$`) or middle (`gm`); a part of a wrapped row counts
            // from its start, a whole one from the view's left column.
            let along = match end {
                true => width - 1,
                false => width / 2,
            };
            let along = along
                + if hop.to_whole {
                    columns.start as usize
                } else {
                    0
                };
            cursor.goto(hop.to.start);
            let first = first_grapheme(&cursor.line, hop.to.start);
            let stop = first_grapheme(&cursor.line, hop.to.end);
            let (mut ix, mut start) = (first, 0);
            while ix < stop && start + cursor.line.width(ix, start, tab) <= along {
                start += cursor.line.width(ix, start, tab);
                ix += 1;
            }
            // Past the row's end, a visual cursor stands on its end;
            // elsewhere on the last character.
            cursor.ix = match ix >= stop && hop.to_last {
                true if visual => cursor.line.len(),
                _ => ix.min(stop.saturating_sub(1)).min(cursor.line.last()),
            };
            match end && hop.rows < rows {
                true => failed(cursor, offset, false, visual, None),
                false => Ok(None),
            }
        }
        Motion::DisplayDown | Motion::DisplayUp => {
            let down = args.motion == Motion::DisplayDown;
            let offset = cursor.offset();
            let hop = VIEW.with(|view| {
                let view = view.borrow();
                let hops = view.as_ref().map_or(&[][..], |view| &view.hops[..]);
                let found = hops.binary_search_by_key(&offset, |hop| hop.offset);
                found.ok().map(|ix| hops[ix].clone())
            });
            let hop = hop.unwrap_or_else(|| row_hop(cursor.text, offset, row, down, count));
            display_rows(cursor, &hop, count, visual, goal, tab)
        }
        _ => Err(goal),
    }
}

/// Notes for the [`with_view`] running that `gj` or `gk` left the caret at
/// `head` with buffer goal `goal`, aiming for display column `along`.
fn record_along(head: usize, goal: usize, along: usize) {
    VIEW.with(|view| {
        if let Some(view) = view.borrow_mut().as_mut() {
            view.alongs.push(DisplayGoal { head, goal, along });
        }
    });
}

/// The display columns the `gj`s and `gk`s of the command running left its
/// carets aiming for: [`crate::motion::apply`] keeps them with the goals it
/// sets, which is how every other motion clears them.
pub(crate) fn alongs() -> Vec<DisplayGoal> {
    VIEW.with(|view| {
        view.borrow()
            .as_ref()
            .map_or_else(Vec::new, |v| v.alongs.clone())
    })
}

/// In a visual mode, a goal at or past the row's end puts the cursor on the
/// row's end, past its last character, as `j` and `k` do there.
fn past_end(cursor: &mut Cursor, visual: bool, goal: usize, tab: usize) {
    if visual && goal >= cursor.line.start_column(cursor.line.len(), tab) {
        cursor.ix = cursor.line.len();
    }
}

/// `ge` (`gE` if `big`) `count` times: each time past the word the caret is
/// on (a row's end ends it), then back over blanks to the end of a word or
/// to an empty row. From past a row's end (`from_end`), the first time
/// starts with the blanks. A step that runs into the start of the text
/// stops there; returns whether one couldn't move at all, which fails the
/// motion (nvim's `ge` from the start, or with steps left on an empty first
/// row).
fn word_ends_back(cursor: &mut Cursor, count: u32, big: bool, from_end: bool) -> bool {
    for step in 0..count {
        if !(from_end && step == 0) {
            let (class, row) = (cursor.class(big), cursor.row);
            let in_word = !matches!(class, Class::Blank | Class::EmptyRow);
            if !cursor.backward() {
                return true;
            }
            while in_word && cursor.row == row && cursor.class(big) == class {
                if !cursor.backward() {
                    return false;
                }
            }
        }
        while cursor.class(big) == Class::Blank {
            if !cursor.backward() {
                return false;
            }
        }
    }
    false
}

/// A motion that failed, with the caret moved from `from` to where it got
/// (`moved` also if a visual cursor past the row's end was clamped): in a
/// visual mode the cursor goes there all the same and the key fails, as
/// nvim's does. Elsewhere it fails where it got to: a normal-mode caret
/// goes there, and so does an operator's, which does nothing
/// ([`moves_when_failing`]).
fn failed(
    cursor: &Cursor,
    from: usize,
    moved: bool,
    visual: bool,
    goal: Option<usize>,
) -> Result<Option<usize>, Option<usize>> {
    if visual && (moved || cursor.offset() != from) {
        report_failed();
        return Ok(goal);
    }
    Err(goal)
}

/// `gj` or `gk` along `hop`: to the grapheme on its last row as far along
/// that display row as the caret is along its own, plus how far the goal is
/// past the caret (so a goal a short row clamped is kept), but no further
/// than a display row may hold, or its last. Columns along a display row
/// count tab stops from its start, as fe-display lays it out. The goal
/// becomes the buffer row's column there (`$`'s stays the row's end). Short
/// of `count` rows it still moves, but fails, as nvim does.
fn display_rows(
    cursor: &mut Cursor,
    hop: &Hop,
    count: u32,
    visual: bool,
    goal: Option<usize>,
    tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    let offset = cursor.offset();
    if hop.rows == 0 {
        // A caret that can't move keeps the display column it held.
        if let (Some(along), Some(goal)) = (hop.along, goal.filter(|&goal| goal != usize::MAX)) {
            record_along(offset, goal, along);
        }
        return Err(goal);
    }
    let goal = goal.unwrap_or_else(|| cursor.line.goal_column(cursor.ix, tab));
    let from = first_grapheme(&cursor.line, hop.from.start);
    let along_start = columns_between(&cursor.line, from, cursor.ix, tab);
    let mut along = match hop.along.filter(|_| goal != usize::MAX) {
        Some(along) => along,
        None => to_along(&cursor.line, cursor.ix, along_start, goal, tab),
    };
    if let Some(limit) = hop.limit.filter(|_| goal != usize::MAX) {
        along = along.min(limit);
    }
    cursor.goto(hop.to.start);
    let first = first_grapheme(&cursor.line, hop.to.start);
    let end = first_grapheme(&cursor.line, hop.to.end).max(first + 1);
    let (mut ix, mut start) = (first, 0);
    while ix + 1 < end && start + cursor.line.width(ix, start, tab) <= along {
        start += cursor.line.width(ix, start, tab);
        ix += 1;
    }
    cursor.ix = ix.min(cursor.line.last());
    let goal = match goal {
        usize::MAX => goal,
        _ => from_along(&cursor.line, cursor.ix, start, along, tab),
    };
    if hop.to_last {
        past_end(cursor, visual, goal, tab);
    }
    if goal != usize::MAX {
        record_along(cursor.offset(), goal, along);
    }
    match hop.rows < count {
        true => failed(cursor, offset, false, visual, Some(goal)),
        false => Ok(Some(goal)),
    }
}

/// Where `goal`, a column of `line` (tab stops from its start), is along
/// the display row grapheme `ix` starts `start` columns into (tab stops
/// from the display row's start). Within that grapheme's columns, as far
/// from its last as there, or its first if it has fewer there (a tab may
/// be narrower or wider on the display row), so its last column, a tab's
/// goal, stays its last; past them, as far past. On an empty row, as far
/// along as the goal.
fn to_along(line: &Line, ix: usize, start: usize, goal: usize, tab: usize) -> usize {
    if ix >= line.len() {
        return start.saturating_add(goal);
    }
    let column = line.start_column(ix, tab);
    let wide = line.width(ix, column, tab).max(1);
    let along_wide = line.width(ix, start, tab).max(1);
    let into = goal.saturating_sub(column);
    match into < wide {
        true => start + along_wide - 1 - (wide - 1 - into).min(along_wide - 1),
        false => (start + along_wide).saturating_add(into - wide),
    }
}

/// [`to_along`] back: the column of `line` `along` is, on the display row
/// grapheme `ix` starts `start` columns into.
fn from_along(line: &Line, ix: usize, start: usize, along: usize, tab: usize) -> usize {
    let into = along.saturating_sub(start);
    if ix >= line.len() {
        return into;
    }
    let column = line.start_column(ix, tab);
    let wide = line.width(ix, column, tab).max(1);
    let along_wide = line.width(ix, start, tab).max(1);
    match into < along_wide {
        true => column + wide - 1 - (along_wide - 1 - into).min(wide - 1),
        false => (column + wide).saturating_add(into - along_wide),
    }
}

/// The display columns graphemes `from..to` of `line` take, with tab stops
/// from `from`.
fn columns_between(line: &Line, from: usize, to: usize, tab: usize) -> usize {
    (from..to).fold(0, |column, ix| column + line.width(ix, column, tab))
}

/// The index of the first grapheme of `line` at or after `offset`.
fn first_grapheme(line: &Line, offset: usize) -> usize {
    let column = offset.saturating_sub(line.start);
    line.starts.partition_point(|&start| start < column)
}
