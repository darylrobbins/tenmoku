//! Scroll: the scroll keys and `vim.scroll` (a feature module; see [`crate::feature`]).
//!
//! In normal and the visual modes `<C-d>` `<C-u>` `<C-e>` `<C-y>` `<C-f>`
//! `<C-b>` (after a count, not an operator, which they cancel) and `zz` `zt`
//! `zb` `z<CR>` `z.` `z-` (`:help scrolling`) commit `vim.scroll
//! {"action", "count"}` ([`ScrollArgs`]). It reads the view from the host's
//! [`EditorHost::viewport`] and asks for a new one through
//! [`EditorHost::scroll`]; a host that doesn't know its view (`None`) is
//! taken as showing `page_rows() + 2` rows from the first. Rows are the
//! text's rows: a fold or a wrapped row doesn't change how far they go.
//! As nvim's, with 'scrolloff' 0:
//!
//! - `<C-d>` and `<C-u>` scroll by 'scroll' rows and move the cursor as
//!   many (as `j` and `k` would, keeping the goal column). 'scroll' is half
//!   the view's rows (rounded down, at least 1); a count sets it for later
//!   uses, at most the view's rows, until the view's height changes.
//!   `<C-d>` doesn't scroll past showing the last row at the bottom, though
//!   the cursor still moves. Even where the cursor's row doesn't change, it
//!   goes to its goal column.
//! - `<C-e>` and `<C-y>` scroll one row (count rows), `<C-e>` until the last
//!   row is at the top; the cursor moves only to stay in view.
//! - `<C-f>` and `<C-b>` scroll a page (count pages) less two rows of
//!   overlap (less in a view under 5 rows, or where the rows kept would be
//!   past the text's end), `<C-f>` until the last row is at the top, and put
//!   the cursor on the view's first (`<C-f>`) or last row (`<C-b>`), at
//!   its goal column.
//! - `zt`, `zz` and `zb` put the cursor's row at the top, centre or bottom
//!   of the view, and `z<CR>`, `z.` and `z-` do too and go to its first
//!   non-blank. With a count they first jump to that row (the jump mark
//!   set, as nvim's do), keeping the cursor's byte column (clamped as a
//!   caret is) and its goal column. In a visual mode (a divergence) the
//!   vim cursor goes to its goal column on that row, as `{count}G` takes
//!   it, where nvim's keeps its byte column: they differ only where the
//!   goal isn't the cursor's column (after `j` through a shorter row).
//!
//! `<C-d>`, `<C-u>`, `<C-f>` and `<C-b>` fail (stopping a macro) when
//! neither the view nor the cursor moves. In a visual mode the vim cursor
//! moves, keeping the anchor.

use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::commands::{EditorHost, ScrollRequest, Viewport};
use fe_text::Bias;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Mode, MoveArgs, Pending, Step, VimCommands, VimKey, VimTarget, args::Motion, feature::Feature,
    marks, motion, reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g: |_, _| None,
    z: Some(z),
    register,
    ..Feature::NONE
};

/// What `vim.scroll` does, by its keys.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAction {
    /// `<C-d>`.
    HalfPageDown,
    /// `<C-u>`.
    HalfPageUp,
    /// `<C-e>`.
    LineDown,
    /// `<C-y>`.
    LineUp,
    /// `<C-f>`.
    PageDown,
    /// `<C-b>`.
    PageUp,
    /// `zt`.
    Top,
    /// `zz`.
    Center,
    /// `zb`.
    Bottom,
    /// `z<CR>`.
    TopFirstNonBlank,
    /// `z.`.
    CenterFirstNonBlank,
    /// `z-`.
    BottomFirstNonBlank,
}

/// `vim.scroll`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScrollArgs {
    pub action: ScrollAction,
    /// The typed count: rows, pages or 'scroll', or the `z` keys' row (from
    /// 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let action = match key {
        VimKey::Ctrl('d') => ScrollAction::HalfPageDown,
        VimKey::Ctrl('u') => ScrollAction::HalfPageUp,
        VimKey::Ctrl('e') => ScrollAction::LineDown,
        VimKey::Ctrl('y') => ScrollAction::LineUp,
        VimKey::Ctrl('f') => ScrollAction::PageDown,
        VimKey::Ctrl('b') => ScrollAction::PageUp,
        _ => return None,
    };
    (!pending.has_operator()).then(|| scroll(pending, action))
}

fn z(pending: &Pending, key: VimKey) -> Option<Step> {
    let action = match key {
        VimKey::Char('t') => ScrollAction::Top,
        VimKey::Char('z') => ScrollAction::Center,
        VimKey::Char('b') => ScrollAction::Bottom,
        VimKey::Enter => ScrollAction::TopFirstNonBlank,
        VimKey::Char('.') => ScrollAction::CenterFirstNonBlank,
        VimKey::Char('-') => ScrollAction::BottomFirstNonBlank,
        _ => return None,
    };
    Some(scroll(pending, action))
}

fn scroll(pending: &Pending, action: ScrollAction) -> Step {
    invocation(
        "vim.scroll",
        ScrollArgs {
            action,
            count: pending.count(),
        },
    )
}

fn register(registry: &mut Registry<VimCommands>) {
    registry
        .register_with_args(
            "vim.scroll",
            "Vim Scroll",
            |target: &mut VimTarget<'_>, _: &Actor, args: ScrollArgs| {
                Ok::<_, CommandError>(run(target, args))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Scrolls the view as vim's ctrl-d, ctrl-u, ctrl-e, ctrl-y, ctrl-f, ctrl-b, zt, zz, zb, z<CR>, z. and z- do, moving the cursor as they do (in a visual mode, the vim cursor). A count sets 'scroll' for ctrl-d and ctrl-u, is rows or pages for the others, and the row to scroll to for the z keys.",
        )
        .context(
            ContextPredicate::parse("editor && (vim.mode == normal || vim.visual)")
                .expect("vim's context predicate parses"),
        )
        .mutates_buffer(false);
}

/// The view a host without one is taken to have: `page_rows() + 2` rows
/// from the first.
pub(crate) fn viewport(host: &dyn EditorHost) -> Viewport {
    host.viewport().unwrap_or(Viewport {
        top_row: 0,
        rows: host.page_rows().saturating_add(2),
    })
}

fn run(target: &mut VimTarget<'_>, args: ScrollArgs) -> Dispatched<fe_buffer::TransactionId> {
    use ScrollAction::*;
    target.model.sync_display();
    let view = viewport(&*target.host);
    let rows = view.rows.max(1);
    let last = target.model.text().row_count().saturating_sub(1);
    let head = target
        .state
        .visual_cursor(target.model)
        .unwrap_or_else(|| target.model.primary_selection().head());
    let row = target.model.text().offset_to_point(head).row;
    let top = view.top_row;
    let count = args.count.filter(|&count| count > 0);
    // A new height drops the counted 'scroll', as nvim's window does.
    if target
        .state
        .scroll
        .is_some_and(|(_, set_for)| set_for != rows)
    {
        target.state.scroll = None;
    }
    // The z keys' row: the count's (from 1), else the cursor's.
    let z_row = count.map_or(row, |count| (count - 1).min(last));
    let (new_top, new_row) = match args.action {
        HalfPageDown | HalfPageUp => {
            if let Some(count) = count {
                target.state.scroll = Some((count.min(rows), rows));
            }
            let amount = match target.state.scroll {
                Some((amount, set_for)) if set_for == rows => amount,
                _ => (rows / 2).max(1),
            };
            if args.action == HalfPageDown {
                // Not past showing the last row at the bottom.
                let room = (last + 1).saturating_sub(top).saturating_sub(rows);
                (top + amount.min(room), row.saturating_add(amount).min(last))
            } else {
                (top.saturating_sub(amount), row.saturating_sub(amount))
            }
        }
        LineDown => {
            let new_top = top.saturating_add(count.unwrap_or(1)).min(last).max(top);
            (new_top, row.max(new_top))
        }
        LineUp => {
            let new_top = top.saturating_sub(count.unwrap_or(1));
            (new_top, row.min(new_top.saturating_add(rows - 1)))
        }
        PageDown => {
            let page = page(rows, top, last, true);
            let new_top = top
                .saturating_add(page.saturating_mul(count.unwrap_or(1)))
                .min(last)
                .max(top);
            (new_top, if new_top == top { row } else { new_top })
        }
        PageUp => {
            let page = page(rows, top, last, false);
            let new_top = top.saturating_sub(page.saturating_mul(count.unwrap_or(1)));
            let bottom = new_top.saturating_add(rows - 1).min(last);
            (new_top, if new_top == top { row } else { bottom })
        }
        Top | TopFirstNonBlank => (z_row, z_row),
        Center | CenterFirstNonBlank => (z_row.saturating_sub((rows - 1) / 2), z_row),
        Bottom | BottomFirstNonBlank => (z_row.saturating_sub(rows - 1), z_row),
    };
    if new_top != top {
        target.host.scroll(ScrollRequest::TopRow(new_top));
    }
    let extend = matches!(target.state.mode, Mode::Visual(_));
    let mut moved = false;
    let z = matches!(
        args.action,
        Top | Center | Bottom | TopFirstNonBlank | CenterFirstNonBlank | BottomFirstNonBlank
    );
    if z && new_row != row {
        moved = jump_to_row(target, head, new_row, extend);
    } else if new_row != row {
        let (motion, by) = match new_row > row {
            true => (Motion::Down, new_row - row),
            false => (Motion::Up, row - new_row),
        };
        let args = MoveArgs {
            motion,
            extend,
            count: Some(by),
        };
        moved = motion::apply(target.model, target.state, args);
    } else if match args.action {
        HalfPageDown | HalfPageUp => true,
        PageDown | PageUp => new_top != top,
        _ => false,
    } && (extend || target.model.selections().len() == 1)
    {
        // Staying on its row, the cursor still goes to its goal column, as
        // nvim's does (it differs only after `{count}zt` and the like):
        // `{row + 1}G` keeps the goal. Several carets keep their places.
        let args = MoveArgs {
            motion: Motion::DocumentEnd,
            extend,
            count: Some(row + 1),
        };
        moved = motion::apply(target.model, target.state, args);
    }
    if matches!(
        args.action,
        TopFirstNonBlank | CenterFirstNonBlank | BottomFirstNonBlank
    ) {
        let args = MoveArgs {
            motion: Motion::FirstNonBlank,
            extend,
            count: None,
        };
        moved |= motion::apply(target.model, target.state, args);
    }
    let scrolled = new_top != top;
    if !scrolled && !moved && matches!(args.action, HalfPageDown | HalfPageUp | PageDown | PageUp) {
        // nvim beeps when neither the view nor the cursor moved.
        target.state.failed = true;
    }
    Dispatched::new(if scrolled || moved {
        Effect::Changed
    } else {
        Effect::Nothing
    })
}

/// `{count}zt` and the like going to `row` from `head` (the vim cursor or
/// primary caret), as nvim's do: a jump, keeping the byte column (clamped as
/// a caret is) and the goal column. In a visual mode, to the goal column:
/// `visual::move_cursor` would reset the goal after a jump to the byte
/// column, and clamp it short of the row's end; it goes to the same byte
/// column (the row's end at most) and keeps the goal, as in other modes.
fn jump_to_row(target: &mut VimTarget<'_>, head: usize, row: u32, extend: bool) -> bool {
    if extend {
        let text = target.model.text().clone();
        let column = text.offset_to_point(head).column;
        let offset = text.row_start(row) + column.min(text.row_len(row)) as usize;
        let offset = text.clip_offset(offset, Bias::Left);
        marks::jump_here(target);
        let moved = crate::visual::jump_cursor(target.model, target.state, offset);
        marks::settle(target);
        return moved;
    }
    let text = target.model.text().clone();
    let version = target.model.snapshot().version();
    let column = text.offset_to_point(head).column;
    let offset = text.row_start(row) + column.min(text.row_len(row)) as usize;
    let offset = text.clip_offset(offset, Bias::Left);
    // The goal `j` and `k` would have kept, as a display column.
    let before = target.model.selections();
    let goal = motion::valid_goals(target.state, version, &before)
        .and_then(|(_, goals)| goals.iter().find(|(at, _)| *at == head))
        .and_then(|(_, goal)| *goal)
        .unwrap_or_else(|| {
            let cursor = motion::Cursor::at(&text, head);
            cursor
                .line
                .goal_column(cursor.ix, target.model.tab_size().max(1))
        });
    marks::jump_here(target);
    let args = MoveArgs {
        motion: Motion::MarkAt {
            offset,
            exact: true,
        },
        extend,
        count: None,
    };
    let moved = motion::apply(target.model, target.state, args);
    marks::settle(target);
    if let Some((_, goals)) = target.state.goals.as_mut() {
        for (_, kept) in goals {
            *kept = Some(goal);
        }
    }
    moved
}

/// How far `<C-f>` (`forward`) or `<C-b>` scrolls one page from `top`, as
/// nvim's do with every row one screen row high: the view's rows less two rows of overlap, less overlap where the
/// view is small or the rows it would keep aren't there, and the whole
/// view where the last row (`<C-f>`) or the first (`<C-b>`) shows.
fn page(rows: u32, top: u32, last: u32, forward: bool) -> u32 {
    let (rows, top, last) = (i64::from(rows), i64::from(top), i64::from(last));
    let whole = rows as u32;
    if (forward && top + rows > last) || (!forward && top == 0) {
        return whole;
    }
    let min = rows - 2;
    // The rows kept in view: the one past the view's edge, then back in.
    let (first, step) = match forward {
        true => (top + rows, -1),
        false => (top - 1, 1),
    };
    let exists = |at: i64| (0..=last).contains(&(first + at * step));
    if !exists(0) || 1 > min || !exists(1) || 2 > min || !exists(2) {
        return whole;
    }
    if !exists(3) || 3 > min {
        return whole - 1;
    }
    whole - 2
}
