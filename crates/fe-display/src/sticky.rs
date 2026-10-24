//! Sticky scroll, as VS Code's: the header rows of the scopes enclosing the
//! topmost visible row stay pinned at the top of an editor, the innermost
//! pushed up as its scope scrolls out.
//!
//! Scopes are buffer row ranges, `header..=last`, from whatever model finds
//! them: fe-syntax's outline, its fold candidates, or [`indent_scopes`]
//! where there's no tree. Placement is in screen terms, so folds, soft wrap
//! and block rows all count.

#[cfg(test)]
use std::cell::Cell;
use std::ops::{Range, RangeInclusive};

use fe_buffer::Text;

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

/// How far above its rows [`indent_scopes`] looks for headers. Past it a
/// file is too deeply nested in too long a block to scan from a frame, and
/// the headers further up aren't found.
pub const INDENT_SCAN_ROWS: u32 = 50_000;

/// The indented blocks overlapping `rows`, sticky scroll's scopes for text
/// without a tree: `header..=last`, sorted by header, as fe-editor's
/// indented-block folds find them. A block is headed by a row with text and
/// holds the rows below it indented deeper, blank rows among them, though
/// not trailing ones.
///
/// Headers above `rows` are found scanning up from `rows.start` for rows
/// indented less than those below them, until one with no indent or
/// [`INDENT_SCAN_ROWS`] up; ends scanning down no further than row
/// `rows.end`. A block still open there ends at `rows.end`: past the rows,
/// which is all sticky scroll's push-up needs to know. So it never scans the
/// whole of a long file.
pub fn indent_scopes(text: &Text, rows: Range<u32>, tab_size: usize) -> Vec<RangeInclusive<u32>> {
    let row_count = text.row_count();
    if rows.start >= rows.end || rows.start >= row_count {
        return Vec::new();
    }
    // To the row past `rows`, or the file's last.
    let bound = rows.end.min(row_count - 1);
    let below = indents(text, rows.start..bound + 1, tab_size);
    // The level the rows start at, their first text row's, or with none
    // before `bound`, past the row above's.
    let level = match below.iter().find_map(|indent| *indent) {
        Some(level) => level,
        None if bound == row_count - 1 => return Vec::new(),
        None => usize::MAX,
    };
    // The rows heading blocks open at `rows.start`, with their indents,
    // outermost first, and the last text row above it, read a window at a
    // time, each four times the last.
    let mut open: Vec<(u32, usize)> = Vec::new();
    let mut last_text_row = None;
    let mut under = level;
    let (mut end, mut window) = (rows.start, 64);
    let top = rows.start.saturating_sub(INDENT_SCAN_ROWS);
    while under > 0 && end > top {
        let start = end.saturating_sub(window).max(top);
        let indents = indents(text, start..end, tab_size);
        for (row, indent) in (start..end).zip(indents).rev() {
            let Some(indent) = indent else {
                continue;
            };
            last_text_row.get_or_insert(row);
            if indent < under {
                open.push((row, indent));
                under = indent;
                if under == 0 {
                    break;
                }
            }
        }
        (end, window) = (start, window * 4);
    }
    open.reverse();
    let mut last_text_row = last_text_row.unwrap_or(0);
    let mut scopes = Vec::new();
    let mut close = |open: &mut Vec<(u32, usize)>, level: usize, last_text_row: u32| {
        while let Some(&(header, _)) = open.last().filter(|(_, indent)| *indent >= level) {
            open.pop();
            if last_text_row > header {
                scopes.push(header..=last_text_row);
            }
        }
    };
    for (row, indent) in (rows.start..=bound).zip(below) {
        let Some(level) = indent else {
            continue;
        };
        close(&mut open, level, last_text_row);
        open.push((row, level));
        last_text_row = row;
    }
    if bound == row_count - 1 {
        close(&mut open, 0, last_text_row);
    } else {
        // Still open past the rows. One headed by the last text row may
        // have no rows under it at all.
        let past = (open.iter()).filter(|(header, _)| *header < last_text_row);
        scopes.extend(past.map(|(header, _)| *header..=rows.end));
    }
    scopes.sort_by_key(|scope| *scope.start());
    scopes
}

/// The column each of `rows` has its text start at, tabs advancing to the
/// next multiple of `tab_size`, or `None` for a blank row, from one pass
/// over their text that skips each row's once its indent is known.
fn indents(text: &Text, rows: Range<u32>, tab_size: usize) -> Vec<Option<usize>> {
    #[cfg(test)]
    MEASURED.set(MEASURED.get() + rows.len());
    let tab_size = tab_size.max(1);
    let end = match rows.end < text.row_count() {
        true => text.row_start(rows.end),
        false => text.len(),
    };
    let mut indents = Vec::with_capacity(rows.len());
    // The row's indent so far, while only whitespace has been read.
    let mut leading = Some(0);
    for chunk in text.chunks_in_range(text.row_start(rows.start)..end) {
        let mut rest = chunk;
        while !rest.is_empty() {
            let Some(mut column) = leading else {
                // The rest of a row whose indent is known.
                let Some(newline) = rest.find('\n') else {
                    break;
                };
                rest = &rest[newline + 1..];
                leading = Some(0);
                continue;
            };
            let mut read = rest.len();
            for (ix, ch) in rest.char_indices() {
                match ch {
                    '\n' => {
                        indents.push(None);
                        column = 0;
                        read = ix + 1;
                        break;
                    }
                    ' ' => column += 1,
                    '\t' => column += tab_size - column % tab_size,
                    ch if ch.is_whitespace() => {}
                    _ => {
                        indents.push(Some(column));
                        leading = None;
                        read = ix;
                        break;
                    }
                }
            }
            if leading.is_some() {
                leading = Some(column);
            }
            rest = &rest[read..];
        }
    }
    // The last row, with no newline after it.
    if leading.is_some() && indents.len() < rows.len() {
        indents.push(None);
    }
    indents
}

#[cfg(test)]
thread_local! {
    /// How many rows [`indent_scopes`] has measured on this thread.
    static MEASURED: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
#[path = "sticky_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sticky_indent_tests.rs"]
mod indent_tests;
