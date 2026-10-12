//! Indent guides: which rows and columns of a text get a vertical guide,
//! computed from the text alone so a painter only has to draw them.
//!
//! A row's indent is the columns of its leading spaces and tabs. A blank row
//! takes the greater indent of the nearest non-blank rows around it, so a
//! block's guides run through its blank lines. A guide at column
//! `k * indent_width` covers each maximal run of rows indented deeper than that
//! column.

use std::ops::Range;

use fe_buffer::Text;

/// Only this many bytes of a row are read for its indent.
const MAX_INDENT_BYTES: usize = 400;
/// A blank row looks at most this many rows each way for a non-blank one.
const LOOK_AROUND: u32 = 1_000;
const DEFAULT_WIDTH: u32 = 4;

/// A vertical guide at `column` over `rows`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndentGuide {
    pub column: u32,
    pub rows: Range<u32>,
}

/// The guides over `rows`, sorted by column then first row, with each guide's
/// rows clipped to `rows`. `indent_width` 0 means `tab_width`; both 0, 4.
pub fn indent_guides(
    text: &Text,
    rows: Range<u32>,
    indent_width: u32,
    tab_width: u32,
) -> Vec<IndentGuide> {
    let (width, tab) = widths(indent_width, tab_width);
    let rows = rows.start..rows.end.min(text.row_count());
    if rows.start >= rows.end {
        return Vec::new();
    }
    let (_, effective) = indents(text, rows.clone(), tab);
    let deepest = effective.iter().copied().max().unwrap_or(0);
    let mut guides = Vec::new();
    let mut column = 0;
    while column < deepest {
        let mut run_start = None;
        for (i, &indent) in effective.iter().enumerate() {
            match (indent > column, run_start) {
                (true, None) => run_start = Some(i),
                (false, Some(start)) => {
                    guides.push(guide(column, rows.start, start, i));
                    run_start = None;
                }
                _ => {}
            }
        }
        if let Some(start) = run_start {
            guides.push(guide(column, rows.start, start, effective.len()));
        }
        column += width;
    }
    guides
}

/// The guide to highlight for a caret on `caret_row`: when the next non-blank
/// row is indented deeper than the caret's row, the guide starting on the row
/// after it; otherwise the innermost guide containing the row. Its rows are
/// not clipped, but reach at most 1,000 rows each way.
pub fn active_indent_guide(
    text: &Text,
    caret_row: u32,
    indent_width: u32,
    tab_width: u32,
) -> Option<IndentGuide> {
    let (width, tab) = widths(indent_width, tab_width);
    if caret_row >= text.row_count() {
        return None;
    }
    let first = caret_row.saturating_sub(LOOK_AROUND);
    let window = first..(caret_row + LOOK_AROUND + 1).min(text.row_count());
    let (raw, effective) = indents(text, window.clone(), tab);
    let at = (caret_row - first) as usize;
    let caret_indent = effective[at];

    let opening = raw[at + 1..]
        .iter()
        .flatten()
        .next()
        .copied()
        .filter(|&next| next > caret_indent)
        .map(|next| (next - 1) / width * width)
        .filter(|&column| column >= caret_indent);
    let (column, row) = match opening {
        Some(column) if at + 1 < effective.len() => (column, at + 1),
        _ => {
            if caret_indent == 0 {
                return None;
            }
            ((caret_indent - 1) / width * width, at)
        }
    };

    let mut start = row;
    while start > 0 && effective[start - 1] > column {
        start -= 1;
    }
    let mut end = row + 1;
    while end < effective.len() && effective[end] > column {
        end += 1;
    }
    Some(guide(column, first, start, end))
}

fn guide(column: u32, base: u32, start: usize, end: usize) -> IndentGuide {
    IndentGuide {
        column,
        rows: base + start as u32..base + end as u32,
    }
}

fn widths(indent_width: u32, tab_width: u32) -> (u32, u32) {
    let tab = if tab_width == 0 {
        DEFAULT_WIDTH
    } else {
        tab_width
    };
    let width = if indent_width == 0 { tab } else { indent_width };
    (width, tab)
}

/// The indent of `row`, or `None` when it is blank.
fn row_indent(text: &Text, row: u32, tab: u32) -> Option<u32> {
    let start = text.row_start(row);
    let len = text.row_len(row) as usize;
    let cap = MAX_INDENT_BYTES as u64;
    let tab = u64::from(tab);
    let mut column = 0u64;
    let mut in_prefix = true;
    let mut read = 0;
    for chunk in text.chunks_in_range(start..start + len) {
        for ch in chunk.chars() {
            if read >= MAX_INDENT_BYTES {
                // Whitespace to the end of what was read, with more after it.
                return Some(column.min(cap) as u32)
                    .map(|c| if in_prefix { cap as u32 } else { c });
            }
            read += ch.len_utf8();
            match ch {
                ' ' if in_prefix => column += 1,
                '\t' if in_prefix => column = (column / tab + 1) * tab,
                c if c.is_whitespace() => in_prefix = false,
                _ => return Some(column.min(cap) as u32),
            }
            column = column.min(cap);
        }
    }
    None
}

/// The raw indent (`None` for blank rows) and the effective indent (blank
/// rows taking their neighbours') of each row in `rows`.
fn indents(text: &Text, rows: Range<u32>, tab: u32) -> (Vec<Option<u32>>, Vec<u32>) {
    let raw: Vec<Option<u32>> = rows.clone().map(|row| row_indent(text, row, tab)).collect();
    let mut above = vec![0; raw.len()];
    let mut below = vec![0; raw.len()];

    // Nearest non-blank row above/below as (row, indent); outside the range
    // it is looked up once, on first need.
    let mut outside: Option<Option<(u32, u32)>> = None;
    let mut last = None;
    for (i, indent) in raw.iter().enumerate() {
        let row = rows.start + i as u32;
        match indent {
            Some(indent) => last = Some((row, *indent)),
            None => {
                let found = last.or_else(|| {
                    *outside.get_or_insert_with(|| {
                        (rows.start.saturating_sub(LOOK_AROUND)..rows.start)
                            .rev()
                            .find_map(|r| row_indent(text, r, tab).map(|i| (r, i)))
                    })
                });
                above[i] = found
                    .filter(|&(r, _)| row - r <= LOOK_AROUND)
                    .map_or(0, |f| f.1);
            }
        }
    }
    let mut outside: Option<Option<(u32, u32)>> = None;
    let mut last = None;
    for (i, indent) in raw.iter().enumerate().rev() {
        let row = rows.start + i as u32;
        match indent {
            Some(indent) => last = Some((row, *indent)),
            None => {
                let found = last.or_else(|| {
                    *outside.get_or_insert_with(|| {
                        (rows.end..(rows.end + LOOK_AROUND).min(text.row_count()))
                            .find_map(|r| row_indent(text, r, tab).map(|i| (r, i)))
                    })
                });
                below[i] = found
                    .filter(|&(r, _)| r - row <= LOOK_AROUND)
                    .map_or(0, |f| f.1);
            }
        }
    }

    let effective = raw
        .iter()
        .enumerate()
        .map(|(i, indent)| indent.unwrap_or_else(|| above[i].max(below[i])))
        .collect();
    (raw, effective)
}

#[cfg(test)]
#[path = "indent_guides_tests.rs"]
mod tests;
