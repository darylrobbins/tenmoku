//! Git conflict markers in a file's text, read the way VS Code's merge-conflict
//! extension reads them (`mergeConflictParser.ts`).
//!
//! [`parse`] scans whole lines (`\n` or `\r\n` ends one; a last line without a
//! break counts) and returns each complete [`Conflict`]:
//!
//! - a line starting with `<<<<<<<` opens a conflict; one met while another is
//!   still open drops the open one and ends the scan, so the conflicts before it
//!   stay and the rest of the text has none;
//! - inside an open conflict and before its splitter, a line starting with
//!   `|||||||` starts a common-ancestor (diff3 / zdiff3) section; several are
//!   allowed and each is kept;
//! - inside an open conflict with no splitter yet, a line starting with
//!   `=======` is the splitter;
//! - inside an open conflict with a splitter, a line starting with `>>>>>>>`
//!   closes it and the conflict is kept;
//! - a conflict still open at the end of the text is dropped, and every other
//!   line is content. In particular a `=======` or `>>>>>>>` outside a conflict
//!   is content (a Markdown setext heading stays one).
//!
//! Labels are free text: whatever follows the seven marker characters and one
//! space, trimmed (`HEAD`, `feature`, `parent of 1a2b3c4 (msg)`). Every range is
//! in bytes of the parsed text. Pure text; no git process runs.

use std::ops::Range;

/// One marker line: the whole line (its line break included) and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub line: Range<usize>,
    pub label: String,
}

/// A complete conflict. `current`, each base's range and `incoming` are the
/// content lines between the markers, line breaks included; empty when a side
/// has no lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub start: Marker,
    pub current: Range<usize>,
    pub bases: Vec<(Marker, Range<usize>)>,
    pub splitter: Range<usize>,
    pub incoming: Range<usize>,
    pub end: Marker,
}

impl Conflict {
    /// From the start marker's first byte to the end marker's last.
    pub fn range(&self) -> Range<usize> {
        self.start.line.start..self.end.line.end
    }
}

struct Open {
    start: Marker,
    bases: Vec<(Marker, usize)>,
    splitter: Option<Range<usize>>,
}

/// Every complete conflict in `text`, in order.
pub fn parse(text: &str) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    let mut open: Option<Open> = None;
    let mut pos = 0;
    while pos < text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
        let line = &text[pos..end];
        let range = pos..end;
        pos = end;
        if line.starts_with("<<<<<<<") {
            if open.is_some() {
                return conflicts;
            }
            open = Some(Open {
                start: marker(line, range),
                bases: Vec::new(),
                splitter: None,
            });
        } else if let Some(o) = open.as_mut() {
            if o.splitter.is_none() && line.starts_with("|||||||") {
                o.bases.push((marker(line, range.clone()), range.end));
            } else if o.splitter.is_none() && line.starts_with("=======") {
                o.splitter = Some(range);
            } else if o.splitter.is_some() && line.starts_with(">>>>>>>") {
                let o = open.take().expect("open conflict");
                let splitter = o.splitter.clone().expect("splitter");
                conflicts.push(finish(o, splitter, marker(line, range)));
            }
        }
    }
    conflicts
}

fn finish(o: Open, splitter: Range<usize>, end: Marker) -> Conflict {
    let mut stops: Vec<usize> = o.bases.iter().map(|(m, _)| m.line.start).collect();
    stops.push(splitter.start);
    let current = o.start.line.end..stops[0];
    let bases = o
        .bases
        .into_iter()
        .zip(stops.iter().skip(1))
        .map(|((m, from), &to)| (m, from..to))
        .collect();
    Conflict {
        start: o.start,
        current,
        bases,
        incoming: splitter.end..end.line.start,
        splitter,
        end,
    }
}

fn marker(line: &str, range: Range<usize>) -> Marker {
    let rest = &line[7..];
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    Marker {
        line: range,
        label: rest.trim().to_string(),
    }
}
