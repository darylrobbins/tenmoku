//! Notebook cells in a plain `.py` file: the `# %%` cell markers that VS
//! Code, Jupytext and Spyder read, parsed into [`Cell`]s. Pure: no GPUI, no
//! buffer, just the file's text. The kernel that runs a cell's code is
//! [`crate::notebook_kernel`].
//!
//! A cell marker is a line that starts, at column 0, with `#`, optional
//! spaces or tabs and `%%` (`# %%`, and Spyder's `#%%`), followed by the end
//! of the line or by whitespace and a title. `# %% [markdown]` (or
//! `# %% [md]`) starts a markdown cell, whose body is comment lines; any
//! other marker starts a code cell. `# %%%`, `# %%x`, an indented marker and
//! a marker further along a line aren't markers.
//!
//! A marker inside a string isn't one either. That's decided by a line
//! scan that tracks which string, if any, a line starts inside of, rather
//! than by asking the syntax tree: the parser stays pure and runs on any
//! text, including a file whose tree isn't parsed yet, and the only
//! constructs that can hide a whole line, a triple-quoted string and a
//! one-quote string continued by a backslash at a line's end, are all it
//! has to know about. Comments are skipped, and one-line strings, so a
//! `"""` inside them doesn't count; a backslash escapes the next character,
//! as it does for quotes in every kind of string, raw ones included.
//!
//! Text before the first marker is a code cell, the preamble, if it isn't
//! blank. A file with no marker has no cells: the feature is off for it.

use std::ops::Range;

/// What a cell holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// Python, run in the file's kernel.
    Code,
    /// Comment lines of markdown, which running does nothing to.
    Markdown,
}

/// One cell of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The row of its `# %%` marker: `None` for the preamble, the code
    /// before the first marker.
    pub marker_row: Option<u32>,
    /// Its body's rows, the marker's excluded: up to the next marker, or to
    /// the file's last row.
    pub rows: Range<u32>,
    pub kind: CellKind,
    /// What follows the marker (and `[markdown]`), if anything does.
    pub title: Option<String>,
}

/// The code a cell runs, and where it is in its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellSource {
    /// The cell's rows, its trailing blank rows trimmed, joined by `\n`:
    /// empty for a markdown cell, or a cell with nothing in it.
    pub code: String,
    /// The 1-based file line of the code's first row, so tracebacks name the
    /// file's own lines.
    pub first_line: u32,
}

/// The cells of `text`, in order: none if it has no marker.
pub fn parse_cells(text: &str) -> Vec<Cell> {
    let mut markers = Vec::new();
    let mut in_string = None;
    let mut rows = 0;
    for (row, line) in lines(text).enumerate() {
        rows = row + 1;
        if in_string.is_none()
            && let Some(marker) = marker(line)
        {
            markers.push((row as u32, marker));
            continue;
        }
        in_string = scan(line, in_string);
    }
    let Some(&(first, _)) = markers.first() else {
        return Vec::new();
    };
    let rows = rows as u32;
    let mut cells = Vec::with_capacity(markers.len() + 1);
    if lines(text).take(first as usize).any(|line| !is_blank(line)) {
        cells.push(Cell {
            marker_row: None,
            rows: 0..first,
            kind: CellKind::Code,
            title: None,
        });
    }
    for (ix, (row, (kind, title))) in markers.iter().enumerate() {
        let end = markers.get(ix + 1).map_or(rows, |(next, _)| *next);
        cells.push(Cell {
            marker_row: Some(*row),
            rows: row + 1..end,
            kind: *kind,
            title: title.clone(),
        });
    }
    cells
}

/// The cell whose marker or body holds `row`.
pub fn cell_at_row(cells: &[Cell], row: u32) -> Option<&Cell> {
    cells
        .iter()
        .find(|cell| cell.marker_row == Some(row) || cell.rows.contains(&row))
}

/// The code `cell` of `text` runs: its rows, trailing blank rows trimmed,
/// and the file line of the first. A markdown cell runs nothing: its code is
/// empty.
pub fn cell_source(cell: &Cell, text: &str) -> CellSource {
    let first_line = cell.rows.start + 1;
    if cell.kind == CellKind::Markdown {
        return CellSource {
            code: String::new(),
            first_line,
        };
    }
    let rows = body(cell, text);
    CellSource {
        code: rows.join("\n"),
        first_line,
    }
}

/// A markdown cell's text: its rows with the leading `# ` (or `#`) of each
/// comment line stripped, trailing blank rows trimmed. Empty for a code
/// cell.
pub fn markdown_text(cell: &Cell, text: &str) -> String {
    if cell.kind != CellKind::Markdown {
        return String::new();
    }
    let rows: Vec<&str> = body(cell, text)
        .into_iter()
        .map(|row| {
            row.strip_prefix("# ")
                .or_else(|| row.strip_prefix('#'))
                .unwrap_or(row)
        })
        .collect();
    rows.join("\n")
}

/// `cell`'s rows of `text`, trailing blank ones trimmed.
fn body<'a>(cell: &Cell, text: &'a str) -> Vec<&'a str> {
    let mut rows: Vec<&str> = lines(text)
        .skip(cell.rows.start as usize)
        .take(cell.rows.len())
        .collect();
    while rows.last().is_some_and(|row| is_blank(row)) {
        rows.pop();
    }
    rows
}

/// `text`'s rows, without their line endings: one more than its newlines.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The kind and title of the cell `line` starts, if it's a marker.
fn marker(line: &str) -> Option<(CellKind, Option<String>)> {
    let rest = line.strip_prefix('#')?;
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix("%%")?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim();
    let tagged = ["[markdown]", "[md]"].iter().find_map(|tag| {
        let title = rest.strip_prefix(tag)?;
        // A tag ends the marker, or whitespace parts it from a title.
        (title.is_empty() || title.starts_with([' ', '\t'])).then_some(title)
    });
    let (kind, title) = match tagged {
        Some(title) => (CellKind::Markdown, title.trim()),
        None => (CellKind::Code, rest),
    };
    let title = (!title.is_empty()).then(|| title.to_owned());
    Some((kind, title))
}

/// A string a line ends inside of, which the next line starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    /// A `"""` or `'''` string, of this quote character.
    Triple(u8),
    /// A `"` or `'` string whose line ends in a backslash, which continues it.
    Single(u8),
}

/// The string open at the end of `line`, given the one open at its start.
fn scan(line: &str, open: Option<Open>) -> Option<Open> {
    let bytes = line.as_bytes();
    let mut ix = 0;
    let mut open = open;
    while ix < bytes.len() {
        match open {
            Some(Open::Triple(quote)) => match bytes[ix] {
                b'\\' => ix += 2,
                _ if bytes[ix..].starts_with(&[quote; 3]) => {
                    open = None;
                    ix += 3;
                }
                _ => ix += 1,
            },
            Some(Open::Single(quote)) => match bytes[ix] {
                b'\\' if ix + 1 == bytes.len() => return open,
                b'\\' => ix += 2,
                byte if byte == quote => {
                    open = None;
                    ix += 1;
                }
                _ => ix += 1,
            },
            None => match bytes[ix] {
                b'#' => break,
                quote @ (b'"' | b'\'') if bytes[ix..].starts_with(&[quote; 3]) => {
                    open = Some(Open::Triple(quote));
                    ix += 3;
                }
                quote @ (b'"' | b'\'') => {
                    open = Some(Open::Single(quote));
                    ix += 1;
                }
                _ => ix += 1,
            },
        }
    }
    // A one-line string that reaches the line's end unescaped ends there (a
    // syntax error, which the scan forgives).
    open.filter(|open| matches!(open, Open::Triple(_)))
}

#[cfg(test)]
#[path = "notebook_cells_tests.rs"]
mod tests;
