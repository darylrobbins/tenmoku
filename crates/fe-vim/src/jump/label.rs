//! The jump's matching and labelling, pure: which places two typed chars
//! match ([`targets`]), how labels name them ([`label`]) and, after the
//! first char, what each candidate would be labelled ([`preview`]). The
//! host's overlay shares them.
//!
//! Matching (fastedit's own spec, after leap):
//!
//! - A typed char matches a grapheme whose base char is it regardless of
//!   case: both have the same Unicode simple case fold.
//! - A typed space matches a space, a tab or a line end: `{c}<Space>`
//!   reaches a row's last char, and `<Space><Space>` an empty row (at its
//!   start). The first char is always a grapheme on a row with text, or an
//!   empty row's start.
//! - A pair of the same char (`==`) matches a run (`=====`) only at its
//!   start. Chars are the same only when case folding makes them so: the
//!   space's wildcard plays no part, so a space and a tab (either order)
//!   are a distinct pair, matched wherever they occur.
//! - The match is at `{c1}`'s grapheme.

use std::{collections::HashMap, ops::Range};

use fe_text::Text;
use regex_syntax::hir::{ClassUnicode, ClassUnicodeRange};

use crate::motion::Line;

/// The labels that can't be mistaken for the start of a command after an
/// autojump, in the order they're given.
pub const SAFE: &str = "sfnut/SFNLHMUGTZ?";

/// Every label, in the order they're given.
pub const FULL: &str = "sfnjklhodweimbuyvrgtaqpcxz/SFNJKLHODWEIMBUYVRGTAQPCXZ?";

/// The matches of `chars` in `rows` after `caret` (before it, if not
/// `forward`), as byte offsets, nearest first in that direction.
pub fn targets(
    text: &Text,
    rows: Range<u32>,
    caret: usize,
    chars: [char; 2],
    forward: bool,
) -> Vec<usize> {
    matches(text, rows, caret, chars, forward).collect()
}

/// [`targets`] over the whole text, lazily.
pub(crate) fn matches(
    text: &Text,
    rows: Range<u32>,
    caret: usize,
    chars: [char; 2],
    forward: bool,
) -> impl Iterator<Item = usize> {
    let rows = rows.start..rows.end.min(text.row_count());
    let row = text.offset_to_point(caret.min(text.len())).row;
    let rows: Box<dyn Iterator<Item = u32>> = match forward {
        true => Box::new(rows.start.max(row)..rows.end),
        false => Box::new((rows.start..rows.end.min(row.saturating_add(1))).rev()),
    };
    rows.flat_map(move |row| {
        let mut found = on_row(&Line::new(text, row), chars);
        if !forward {
            found.reverse();
        }
        found
    })
    .filter(move |&offset| match forward {
        true => offset > caret,
        false => offset < caret,
    })
}

/// The matches on one row, first to last.
fn on_row(line: &Line, [c1, c2]: [char; 2]) -> Vec<usize> {
    if line.len() == 0 {
        return match c1 == ' ' && c2 == ' ' {
            true => vec![line.start],
            false => Vec::new(),
        };
    }
    let (c1, c2) = (Typed::new(c1), Typed::new(c2));
    let at = |ix: usize| (ix < line.len()).then(|| line.grapheme(ix));
    // Same by case folding alone: a space and a tab are a distinct pair.
    let same = c1.folds_with(c2.ch);
    (0..line.len())
        .filter(|&ix| {
            c1.takes(at(ix)) && c2.takes(at(ix + 1)) && !(same && ix > 0 && c1.takes(at(ix - 1)))
        })
        .map(|ix| line.start + line.starts[ix])
        .collect()
}

/// After `{c1}`: each match of it in `rows` from `caret` (in the
/// direction), with the label it gets once the char after it is typed as
/// `{c2}` (`<Space>` for a blank or the row's end), in its group: the
/// targets of that pair in `rows`, labelled after an operator if
/// `op_pending`. The label is `None` for the match its group would jump
/// straight to. A match its group doesn't take (inside a same-char run) is
/// left out. Rows `shown` says no to (hidden in a fold) are left out of
/// both. In text order.
pub fn preview(
    text: &Text,
    rows: Range<u32>,
    shown: &dyn Fn(u32) -> bool,
    caret: usize,
    c1: char,
    forward: bool,
    op_pending: bool,
) -> Vec<(usize, Option<Label>)> {
    let rows = rows.start..rows.end.min(text.row_count());
    let typed = Typed::new(c1);
    // Each group's labels, by offset.
    let mut groups: HashMap<char, HashMap<usize, Option<Label>>> = HashMap::new();
    let mut found = Vec::new();
    for row in rows.clone().filter(|&row| shown(row)) {
        let line = Line::new(text, row);
        let candidates: Vec<(usize, char)> = match line.len() {
            0 if c1 == ' ' => vec![(line.start, ' ')],
            0 => Vec::new(),
            len => (0..len)
                .filter(|&ix| typed.takes(Some(line.grapheme(ix))))
                .map(|ix| {
                    let next = (ix + 1 < len).then(|| line.grapheme(ix + 1));
                    let c2 = match next.and_then(|grapheme| grapheme.chars().next()) {
                        None | Some('\t') => ' ',
                        Some(ch) => ch,
                    };
                    (line.start + line.starts[ix], c2)
                })
                .collect(),
        };
        for (offset, c2) in candidates {
            let group = groups.entry(c2).or_insert_with(|| {
                let mut targets = targets(text, rows.clone(), caret, [c1, c2], forward);
                targets.retain(|&offset| shown(text.offset_to_point(offset).row));
                let labeled = label(targets.len(), op_pending);
                let labels = targets.into_iter().zip(labeled.labels);
                labels.collect()
            });
            if let Some(&label) = group.get(&offset) {
                found.push((offset, label));
            }
        }
    }
    found
}

/// A typed char and the chars it matches: its Unicode simple case folding
/// orbit (CaseFolding.txt's `C` and `S` mappings, as `regex-syntax` holds
/// them), so `Σ σ ς`, `S s ſ`, `ΐ ΐ` and `K k` with the Kelvin sign each
/// match alike, and `ı` matches only itself.
struct Typed {
    ch: char,
    variants: ClassUnicode,
}

impl Typed {
    fn new(ch: char) -> Self {
        let mut variants = ClassUnicode::new([ClassUnicodeRange::new(ch, ch)]);
        variants.case_fold_simple();
        Self { ch, variants }
    }

    /// Whether it matches `grapheme`'s base char, or the line end if
    /// `None`.
    fn takes(&self, grapheme: Option<&str>) -> bool {
        let base = grapheme.and_then(|grapheme| grapheme.chars().next());
        match (self.ch, base) {
            (' ', None | Some(' ' | '\t')) => true,
            (_, Some(base)) => self.folds_with(base),
            (_, None) => false,
        }
    }

    /// Whether case folding makes `ch` the same char as this one.
    fn folds_with(&self, ch: char) -> bool {
        self.variants
            .ranges()
            .iter()
            .any(|range| (range.start()..=range.end()).contains(&ch))
    }
}

/// How a session labels its targets ([`label`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labeled {
    /// Whether it goes to the first target at once.
    pub autojump: bool,
    /// Each target's label, in target order: `None` for the first when it
    /// autojumps.
    pub labels: Vec<Option<Label>>,
}

/// A target's label: a char of [`SAFE`] or [`FULL`] on a page.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label {
    pub ch: char,
    /// From 0; only [`FULL`] runs to more than one.
    pub page: usize,
}

/// Labels `count` targets, nearest first, after an operator if
/// `op_pending`. It autojumps when every target after the first fits
/// [`SAFE`] (so always for one), but never after an operator with more than
/// one target; the rest then take [`SAFE`]. Otherwise [`FULL`] labels them
/// all, page after page.
pub fn label(count: usize, op_pending: bool) -> Labeled {
    let safe: Vec<char> = SAFE.chars().collect();
    let autojump = count > 0 && count - 1 <= safe.len() && !(op_pending && count > 1);
    let labels = match autojump {
        true => std::iter::once(None)
            .chain(safe.iter().map(|&ch| Some(Label { ch, page: 0 })))
            .take(count)
            .collect(),
        false => {
            let full: Vec<char> = FULL.chars().collect();
            (0..count)
                .map(|ix| {
                    Some(Label {
                        ch: full[ix % full.len()],
                        page: ix / full.len(),
                    })
                })
                .collect()
        }
    };
    Labeled { autojump, labels }
}
