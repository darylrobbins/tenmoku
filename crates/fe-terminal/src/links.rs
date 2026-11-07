//! The link under a terminal cell, for the view to underline and open: an OSC 8
//! hyperlink, a URL in the text, or a path to a file that exists, with an
//! optional line and column (`src/main.rs:12:5`, `src/main.rs:12.5`,
//! `src/main.rs(12,5)`, `src/main.rs(12, 5)`, `src/main.rs(12:5)`, `src/main.rs[12]`,
//! `src/main.rs[12,5]`, `src/main.rs[12, 5]`, `src/main.rs[12:5]`,
//! `"src/main.py", line 12, column 5`, `file:///src/main.rs:12:5`).
//!
//! Links don't continue across rows: one the terminal soft-wrapped is found
//! only up to the row's edge, and a path cut there may not exist.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::snapshot::{Cell, CellFlags, Snapshot};

/// A link on one row of a [`Snapshot`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub row: usize,
    /// The cells the link covers, to underline.
    pub cols: Range<usize>,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// An absolute path (when the cwd is), from a `file://` URI or a path in
    /// the text, which exists.
    File {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// The link covering cell `col` of viewport row `row`, if any: the cell's OSC
/// 8 hyperlink, else a `http(s)://`, `file://` or `mailto:` URL in the text,
/// else a path that exists, relative paths resolved against `cwd`. Costs one
/// `metadata` call per path candidate under the cell, and no other I/O.
pub fn link_at(snapshot: &Snapshot, row: usize, col: usize, cwd: &Path) -> Option<Link> {
    let cells = snapshot.rows.get(row)?;
    let cell = cells.get(col)?;
    if let Some(index) = cell.link {
        let uri = snapshot.links.get(index as usize)?;
        let start = cells[..col]
            .iter()
            .rposition(|c| c.link != Some(index))
            .map_or(0, |i| i + 1);
        let end = cells[col..]
            .iter()
            .position(|c| c.link != Some(index))
            .map_or(cells.len(), |i| col + i);
        return Some(Link {
            row,
            cols: start..end,
            target: uri_target(uri),
        });
    }

    let text = RowText::new(cells);
    let at = text.index_of(col)?;
    let (span, target) = url_at(&text, at).or_else(|| path_at(&text, at, cwd))?;
    Some(Link {
        row,
        cols: text.cols(span),
        target,
    })
}

/// A row's characters, with the cells each covers (two for a wide one).
struct RowText {
    chars: Vec<char>,
    /// The combining marks of each character, parallel to `chars`.
    marks: Vec<Option<String>>,
    cells: Vec<Range<usize>>,
}

impl RowText {
    fn new(cells: &[Cell]) -> RowText {
        let mut text = RowText {
            chars: Vec::with_capacity(cells.len()),
            marks: Vec::with_capacity(cells.len()),
            cells: Vec::with_capacity(cells.len()),
        };
        for (col, cell) in cells.iter().enumerate() {
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                if let Some(last) = text.cells.last_mut() {
                    last.end = col + 1;
                }
                continue;
            }
            text.chars.push(cell.ch);
            text.marks.push(cell.extra.clone());
            text.cells.push(col..col + 1);
        }
        text
    }

    /// The character covering cell `col`.
    fn index_of(&self, col: usize) -> Option<usize> {
        self.cells.iter().position(|cells| cells.contains(&col))
    }

    /// Characters `span` as text, each followed by its combining marks.
    fn text(&self, span: Range<usize>) -> String {
        let mut text = String::new();
        for i in span {
            text.push(self.chars[i]);
            if let Some(marks) = &self.marks[i] {
                text.push_str(marks);
            }
        }
        text
    }

    /// The cells characters `span` cover.
    fn cols(&self, span: Range<usize>) -> Range<usize> {
        self.cells[span.start].start..self.cells[span.end - 1].end
    }
}

const SCHEMES: [&str; 4] = ["https://", "http://", "file://", "mailto:"];

/// The URL covering character `at`, with its characters.
fn url_at(text: &RowText, at: usize) -> Option<(Range<usize>, LinkTarget)> {
    let chars = &text.chars;
    let mut i = 0;
    while i <= at {
        let after_word = i > 0 && chars[i - 1].is_alphanumeric();
        if !after_word && let Some(scheme) = SCHEMES.iter().find(|s| starts_with(chars, i, s)) {
            let body = i + scheme.len();
            let end = url_end(chars, i, body);
            if end > body {
                if (i..end).contains(&at) {
                    return Some((i..end, with_url_location(uri_target(&text.text(i..end)))));
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Whether `chars[at..]` starts with `prefix`, ignoring ASCII case.
fn starts_with(chars: &[char], at: usize, prefix: &str) -> bool {
    let mut rest = chars[at..].iter();
    prefix
        .chars()
        .all(|p| rest.next().is_some_and(|c| c.eq_ignore_ascii_case(&p)))
}

/// Where the URL starting at `start`, whose scheme ends at `body`, ends: at
/// whitespace, or a closing bracket or quote with no opener inside the URL,
/// with trailing `.,;:!?` dropped.
fn url_end(chars: &[char], start: usize, body: usize) -> usize {
    const OPEN: [char; 3] = ['(', '[', '{'];
    const CLOSE: [char; 3] = [')', ']', '}'];
    const QUOTES: [char; 3] = ['"', '\'', '`'];
    let quoted_by = start.checked_sub(1).map(|i| chars[i]);
    let mut depth = [0usize; 3];
    let mut open_quote = [false; 3];
    let mut end = body;
    while end < chars.len() {
        let c = chars[end];
        if c.is_whitespace() || c.is_control() || c == '<' || c == '>' {
            break;
        }
        if let Some(k) = OPEN.iter().position(|&o| o == c) {
            depth[k] += 1;
        } else if let Some(k) = CLOSE.iter().position(|&o| o == c) {
            if depth[k] == 0 {
                break;
            }
            depth[k] -= 1;
        } else if let Some(k) = QUOTES.iter().position(|&q| q == c) {
            // A quote closes one opened inside the URL; else it closes the one
            // before the URL, or opens one.
            if open_quote[k] {
                open_quote[k] = false;
            } else if quoted_by == Some(c) {
                break;
            } else {
                open_quote[k] = true;
            }
        }
        end += 1;
    }
    while end > body && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?') {
        end -= 1;
    }
    end
}

/// A `file://` URI as a [`LinkTarget::File`]; any other URI as a URL.
fn uri_target(uri: &str) -> LinkTarget {
    let rest = uri
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file://"))
        .map(|_| &uri[7..]);
    // `file://host/path`: the host is usually empty or `localhost`.
    match rest.and_then(|rest| rest.find('/').map(|slash| &rest[slash..])) {
        Some(path) => LinkTarget::File {
            path: PathBuf::from(drive_path(percent_decode(path))),
            line: None,
            column: None,
        },
        None => LinkTarget::Url(uri.to_owned()),
    }
}

/// A `file:` URI's path as a path: on Windows its `/C:/…` is the drive path
/// `C:/…`.
fn drive_path(path: String) -> String {
    let bytes = path.as_bytes();
    let drive = cfg!(windows)
        && bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':';
    if drive { path[1..].to_owned() } else { path }
}

/// A `file://` target whose path ends in `:line[:column]` (a Node.js ES module
/// stack frame), with that suffix split off into `line` and `column`.
fn with_url_location(target: LinkTarget) -> LinkTarget {
    let LinkTarget::File { path, .. } = &target else {
        return target;
    };
    let Some(text) = path.to_str() else {
        return target;
    };
    let chars: Vec<char> = text.chars().collect();
    let split = (0..chars.len()).filter(|&i| chars[i] == ':').find_map(|i| {
        let (line, column, end) = location(&chars, i, false);
        (line.is_some() && end == chars.len()).then_some((i, line, column))
    });
    match split {
        Some((i, line, column)) if i > 0 => LinkTarget::File {
            path: PathBuf::from(chars[..i].iter().collect::<String>()),
            line,
            column,
        },
        _ => target,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '+' | '@' | '%')
}

/// The existing path covering character `at`, with its `:line[:column]` or
/// `(line[,column])` suffix, or `", line N[, column M]` after a quoted one.
fn path_at(text: &RowText, at: usize, cwd: &Path) -> Option<(Range<usize>, LinkTarget)> {
    let chars = &text.chars;
    let mut start = 0;
    while start <= at {
        if !is_path_char(chars[start]) {
            start += 1;
            continue;
        }
        let run_end = (start..chars.len())
            .find(|&i| !is_path_char(chars[i]))
            .unwrap_or(chars.len());
        let quoted = start > 0 && chars[start - 1] == '"';
        let (line, column, end) = location(chars, run_end, quoted);
        if (start..end).contains(&at) {
            let (len, path) = existing_path(text, start..run_end, line, cwd)?;
            let span = if len == run_end - start {
                start..end
            } else {
                start..start + len
            };
            let target = LinkTarget::File { path, line, column };
            return span.contains(&at).then_some((span, target));
        }
        start = end;
    }
    None
}

/// The `:line[:column]`, `:line.column`, `(line[,column])` or `[line[,column]]`
/// (the column after `,`, `, ` or `:`) at `chars[at..]`, or for a
/// `quoted` path the `", line N[, column M]` there, and where it ends (`at`
/// when there is none).
fn location(chars: &[char], at: usize, quoted: bool) -> (Option<u32>, Option<u32>, usize) {
    let number = |from: usize| -> Option<(u32, usize)> {
        let end = (from..chars.len())
            .find(|&i| !chars[i].is_ascii_digit())
            .unwrap_or(chars.len());
        let digits: String = chars[from..end].iter().collect();
        Some((digits.parse().ok()?, end))
    };
    match chars.get(at) {
        Some(':') => {
            let Some((line, end)) = number(at + 1) else {
                return (None, None, at);
            };
            match chars
                .get(end)
                .filter(|&&c| c == ':' || c == '.')
                .and_then(|_| number(end + 1))
            {
                Some((column, end)) => (Some(line), Some(column), end),
                None => (Some(line), None, end),
            }
        }
        Some(&open @ ('(' | '[')) => {
            let close = if open == '(' { ')' } else { ']' };
            let Some((line, end)) = number(at + 1) else {
                return (None, None, at);
            };
            let after_comma = |sep: usize| {
                // `, ` takes exactly one blank.
                number(sep + usize::from(chars.get(sep) == Some(&' ')))
            };
            let (column, end) = match chars
                .get(end)
                .filter(|&&c| matches!(c, ',' | ':'))
                .and_then(|_| after_comma(end + 1))
            {
                Some((column, end)) => (Some(column), end),
                None => (None, end),
            };
            if chars.get(end) == Some(&close) {
                (Some(line), column, end + 1)
            } else {
                (None, None, at)
            }
        }
        Some('"') if quoted => {
            let Some((line, end)) = word_number(chars, at + 1, &["line"]) else {
                return (None, None, at);
            };
            match word_number(chars, end, &["column", "col"]) {
                Some((column, end)) => (Some(line), Some(column), end),
                None => (Some(line), None, end),
            }
        }
        _ => (None, None, at),
    }
}

/// `, word N` at `chars[at..]` (`word` one of `words`, spaces allowed after
/// the comma and required before the number), as `N` and where it ends.
fn word_number(chars: &[char], at: usize, words: &[&str]) -> Option<(u32, usize)> {
    let spaces = |from: usize| {
        (from..chars.len())
            .find(|&i| chars[i] != ' ')
            .unwrap_or(chars.len())
    };
    if chars.get(at) != Some(&',') {
        return None;
    }
    let word = spaces(at + 1);
    words.iter().find_map(|w| {
        let after = word + w.len();
        let matches = w
            .chars()
            .enumerate()
            .all(|(i, c)| chars.get(word + i) == Some(&c));
        let from = spaces(after);
        if !matches || from == after {
            return None;
        }
        let end = (from..chars.len())
            .find(|&i| !chars[i].is_ascii_digit())
            .unwrap_or(chars.len());
        let digits: String = chars[from..end].iter().collect();
        Some((digits.parse().ok()?, end))
    })
}

/// The resolved path of `run`, if it exists, and how many of its characters
/// it takes: all of them, or (with no `line`, so `see src/main.rs.` works)
/// all but trailing dots.
fn existing_path(
    row: &RowText,
    run: Range<usize>,
    line: Option<u32>,
    cwd: &Path,
) -> Option<(usize, PathBuf)> {
    let run_chars = &row.chars[run.clone()];
    let mut len = run_chars.len();
    loop {
        let text = row.text(run.start..run.start + len);
        if text.chars().any(char::is_alphanumeric)
            && let Some(path) = resolve(&text, cwd)
            && std::fs::metadata(&path).is_ok()
        {
            return Some((len, path));
        }
        if line.is_some() || len == 0 || run_chars[len - 1] != '.' {
            return None;
        }
        while len > 0 && run_chars[len - 1] == '.' {
            len -= 1;
        }
    }
}

/// `text` against `cwd`, with `~/` as the home directory and `.` components
/// dropped.
fn resolve(text: &str, cwd: &Path) -> Option<PathBuf> {
    let path = match text.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME")?).join(rest),
        None if text == "~" => PathBuf::from(std::env::var_os("HOME")?),
        None => cwd.join(text),
    };
    Some(path.components().collect())
}
