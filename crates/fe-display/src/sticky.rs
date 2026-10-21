//! Sticky scroll, as VS Code's: the header rows of the scopes enclosing the
//! topmost visible row stay pinned at the top of an editor, the innermost
//! pushed up as its scope scrolls out.
//!
//! Scopes are buffer row ranges, `header..=last`, from whatever model finds
//! them (fe-syntax's outline, for one). Placement is in screen terms, so
//! folds, soft wrap and block rows all count.

use std::ops::RangeInclusive;

use crate::{BlockMap, DisplayMap};

/// A header row pinned at the top of an editor.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StickyRow {
    pub buffer_row: u32,
    /// Pixels from the view's top: `index × line_height`, or less for the
    /// innermost row while its scope's last row scrolls out under it.
    pub y: f32,
}

/// The rows pinned at the top of an editor scrolled to `scroll_y` (in
/// pixels, block rows counted), from `scopes` sorted by header, outermost
/// first.
///
/// Slot `i` of the band holds the next scope whose header's screen row
/// starts strictly above `scroll_y + i × line_height`, and whose last row's
/// bottom is below it. The innermost row is pushed up as its last row
/// scrolls under it: `y = min(i × line_height, bottom of last − scroll_y −
/// line_height)`, and nothing is pinned below a pushed row. A scope whose
/// header is hidden in a closed fold is skipped, a wrapped header takes one
/// slot (its first display row), and a header row is pinned once however
/// many scopes start on it. At most `max_rows` rows, and none whose bottom
/// would pass `keep_above`, a y relative to the view's top such as the
/// caret's, so the band never covers it.
pub fn sticky_rows(
    scopes: &[RangeInclusive<u32>],
    display: &DisplayMap,
    blocks: &BlockMap,
    scroll_y: f32,
    line_height: f32,
    max_rows: usize,
    keep_above: Option<f32>,
) -> Vec<StickyRow> {
    let mut rows: Vec<StickyRow> = Vec::new();
    if line_height <= 0. {
        return rows;
    }
    let text = display.text();
    let last_row = text.row_count() - 1;
    let top = |display_row: u32| blocks.screen_row(display_row) as f32 * line_height;
    for scope in scopes {
        if rows.len() == max_rows {
            break;
        }
        let header = *scope.start();
        if rows.last().is_some_and(|row| row.buffer_row >= header)
            || header > last_row
            || display.hidden_rows(header).is_some()
        {
            continue;
        }
        let slot = rows.len() as f32 * line_height;
        let slot_top = scroll_y + slot;
        if top(display.row_for_buffer_row(header)) >= slot_top {
            continue;
        }
        // The bottom of the last row's last display row, or of the fold
        // header hiding it.
        let last = (*scope.end()).min(last_row);
        let end = text.row_start(last) + text.row_len(last) as usize;
        let bottom = top(display.row_for_offset(end)) + line_height;
        if bottom <= slot_top {
            continue;
        }
        let y = slot.min(bottom - scroll_y - line_height);
        if keep_above.is_some_and(|keep_above| y + line_height > keep_above) {
            break;
        }
        rows.push(StickyRow {
            buffer_row: header,
            y,
        });
        if y < slot {
            break;
        }
    }
    rows
}

#[cfg(test)]
#[path = "sticky_tests.rs"]
mod tests;
