//! [`DiffView`]'s inline mode (VS Code's inline diff editor): only the
//! right side shows, each hunk's removed left rows a tinted block above the
//! right row that takes their place. Switching modes keeps the right side's
//! caret, selection and top row.

use fe_editor::{
    decorations::Tint,
    text_diff::{Side, TextDiff},
};
use gpui::{Context, Focusable as _, SharedString, Window, px};

use super::{DiffView, caret_row, row_offset};
use crate::commands::{HostBlock, HostBlockKind};

/// Below this window width a newly opened [`DiffView`] starts inline.
pub(super) const NARROW: f32 = 900.;

/// Inline, the screen row right row `row` shows on: the row plus the
/// removed rows above it.
pub(super) fn screen_row(diff: &TextDiff, row: u32) -> u32 {
    row + (diff.removed_above().into_iter())
        .take_while(|&(above, _)| above <= row)
        .map(|(_, removed)| removed.len() as u32)
        .sum::<u32>()
}

/// Inline, the right row at `screen`, and whether `screen` is a removed
/// row; in one, the row below the block.
pub(super) fn row_at_screen(diff: &TextDiff, screen: u32) -> (u32, bool) {
    let mut above_rows = 0;
    for (above, removed) in diff.removed_above() {
        let start = above + above_rows;
        if screen < start {
            break;
        }
        let len = removed.len() as u32;
        if screen < start + len {
            return (above, true);
        }
        above_rows += len;
    }
    (screen - above_rows, false)
}

/// The right row paired with left row `row`: in a hunk, the same row in it
/// (its last, past the end); outside one, the row at the same distance from
/// the hunks around.
pub(super) fn paired_right_row(diff: &TextDiff, row: u32) -> u32 {
    if let Some(hunk) = diff.hunk_at(Side::Left, row).map(|at| &diff.hunks()[at])
        && hunk.left.contains(&row)
    {
        let last = hunk.right.end.saturating_sub(1).max(hunk.right.start);
        return (hunk.right.start + (row - hunk.left.start)).min(last);
    }
    let shift: i64 = (diff.hunks().iter())
        .filter(|hunk| hunk.left.end <= row)
        .map(|hunk| hunk.right.len() as i64 - hunk.left.len() as i64)
        .sum();
    (i64::from(row) + shift).max(0) as u32
}

/// The blocks of removed rows above the right side, from the left text.
pub(super) fn removed_blocks(diff: &TextDiff, left: &str) -> Vec<HostBlock> {
    let rows = split_rows(left);
    (diff.removed_above().into_iter().enumerate())
        .map(|(id, (above, removed))| {
            let lines = (removed.clone())
                .map(|row| rows.get(row as usize).copied().unwrap_or(""))
                .map(|line| SharedString::from(line.to_owned()))
                .collect();
            HostBlock {
                above,
                rows: removed.len() as u32,
                kind: HostBlockKind::Lines {
                    lines,
                    tint: Tint::Removed,
                },
                id: id as u64,
            }
        })
        .collect()
}

/// `text`'s rows without their endings: `\r\n`, `\r` and `\n` all end one,
/// as the diff and the editor split them.
fn split_rows(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let (mut rows, mut start, mut i) = (Vec::new(), 0, 0);
    while i < bytes.len() {
        let ending = match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => 0,
        };
        if ending > 0 {
            rows.push(&text[start..i]);
            start = i + ending;
        }
        i += ending.max(1);
    }
    rows.push(&text[start..]);
    rows
}

impl DiffView {
    /// Whether the view shows the right side alone, removed rows inline.
    pub fn is_inline(&self) -> bool {
        self.inline
    }

    /// `diff.toggle_inline`: switches between side by side and inline.
    pub fn toggle_inline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_inline(!self.inline, window, cx);
    }

    /// Shows the view inline or side by side, as the user's choice: it
    /// stays so when the window is narrow. The right side keeps its caret,
    /// selection and top row, and the focus.
    pub fn set_inline(&mut self, inline: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.chosen = true;
        if self.inline == inline {
            return;
        }
        let left_focused = self.views[0].focus_handle(cx).is_focused(window);
        if inline && left_focused {
            let row = self.views[0].read_with(cx, |view, _| caret_row(view.model()));
            let row = paired_right_row(&self.diff, row);
            self.views[1].update(cx, |view, cx| {
                let at = row_offset(view.model().text(), row);
                view.select_and_reveal(at..at, cx);
            });
        }
        // The top row, and how far into it, in the mode left.
        let top = self.views[1].read(cx).scroll_top();
        let whole = top.floor();
        let (row, filler) = match self.inline {
            true => row_at_screen(&self.diff, whole as u32),
            false => self.diff.row_at_screen(Side::Right, whole as u32),
        };
        let part = if filler { 0. } else { top - whole };
        self.inline = inline;
        self.show_blocks(cx);
        let screen = match inline {
            true => screen_row(&self.diff, row),
            false => self.diff.screen_row(Side::Right, row),
        };
        let top = screen as f32 + part;
        for view in &self.views {
            view.update(cx, |view, cx| view.set_scroll_top(top, cx));
        }
        window.focus(&self.views[1].focus_handle(cx), cx);
        cx.notify();
    }

    /// A newly opened view in a window under [`NARROW`] starts inline,
    /// unless the host or user chose a mode first.
    pub(super) fn start_narrow(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !std::mem::replace(&mut self.sized, true)
            && !self.chosen
            && window.viewport_size().width < px(NARROW)
        {
            self.inline = true;
            self.show_blocks(cx);
        }
    }
}
