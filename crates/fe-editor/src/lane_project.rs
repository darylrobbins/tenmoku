//! Three-way projection: a lane's changes to a text (`base` to `lane`) as a
//! proposal over the user's text as it is now (`current`), each hunk placed
//! only where the base→current diff shows its surroundings untouched and
//! unique, never searched for, and everything else stale; plus the
//! whole-file preconditions of new and deleted files
//! (`docs/designs/agent-lanes.md` § Detached hunks, § New and deleted files).
//! GPUI-free.

use std::{collections::HashMap, ops::Range, sync::Arc};

use fe_buffer::{BufferSnapshot, DetachedHunk, Origin, ProposedChange};

use crate::text_diff::TextDiff;

/// `base`→`lane` as hunks in `current`, through [`ProposedChange::from_detached`].
///
/// The hunks are the row diff of `base` and `lane` ([`TextDiff`]). If
/// `current` equals `base` every hunk is pending at its base range. Otherwise
/// a hunk is pending only where [`placement`] places it, and stale for good
/// everywhere else: shown and rejectable, never acceptable, and at offset 0
/// (it has no place in `current`). Line endings are compared as a buffer
/// holds them: `\r\n` and `\r` in `base` and `lane` read as `\n`.
///
/// A side over [`crate::text_diff::MAX_DIFF_BYTES`] isn't diffed: `base` to
/// `lane` is then one hunk over the whole text, and against a `current` too
/// large to diff only an unchanged one places anything.
pub fn lane_project(
    base: &str,
    lane: &str,
    current: &BufferSnapshot,
    origin: Origin,
) -> ProposedChange {
    let current_text = current.text().to_string();
    let hunks = project(base, lane, &current_text)
        .into_iter()
        .map(|hunk| hunk.hunk)
        .collect();
    ProposedChange::from_detached(current, hunks, origin)
        .expect("a placed hunk's window, and so its old text, is unchanged in current")
}

/// One hunk of [`project`]: where it is in the base, and how it comes back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedHunk {
    /// The hunk's old text's range in the base (LF-normalised).
    pub base: Range<usize>,
    /// For [`ProposedChange::from_detached`]: `at` is its range in current,
    /// `None` when stale.
    pub hunk: DetachedHunk,
}

/// The hunks [`lane_project`] proposes, in text order, with their base
/// ranges, against `current`'s text.
pub fn project(base: &str, lane: &str, current: &str) -> Vec<ProjectedHunk> {
    let (base, lane, current) = (lf(base), lf(lane), lf(current));
    let hunks = lane_hunks(&base, &lane);
    if hunks.is_empty() {
        return Vec::new();
    }
    let placement = Placement::new(&base, &current);
    hunks
        .into_iter()
        .map(|(range, new_text)| {
            let at = placement.place(range.clone());
            ProjectedHunk {
                hunk: DetachedHunk {
                    stale: at.is_none(),
                    at,
                    old_text: base[range.clone()].into(),
                    new_text: new_text.into(),
                },
                base: range,
            }
        })
        .collect()
}

/// The placement rule, for ranges of one `base` against one `current`
/// (both LF-normalised).
///
/// A range's **window** is the full rows it touches plus one whole row on
/// each side, or the start or end of the text; for an empty range, the row
/// it is in plus one on each side. If `current` equals `base` every range is
/// placed where it is. Otherwise a range is placed only if its window lies
/// in one unchanged run of the base→current [`TextDiff`], touching no
/// changed rows (a change at the run's edge, or an insertion there, touches
/// it), and the window's text occurs exactly once in `base` and exactly once
/// in `current` (anywhere, at any byte), where the diff maps it.
pub struct Placement<'a> {
    base: Lines<'a>,
    current: Lines<'a>,
    /// The base→current hunks; `None` when the texts are equal.
    diff: Option<DiffRows>,
}

enum DiffRows {
    /// Too large to diff: nothing is placed.
    Unknown,
    /// The hunks' base and current rows, sorted.
    Hunks(Vec<(Range<u32>, Range<u32>)>),
}

impl<'a> Placement<'a> {
    pub fn new(base: &'a str, current: &'a str) -> Self {
        let diff = (base != current).then(|| {
            let diff = TextDiff::new(base, current);
            if diff.too_large() {
                DiffRows::Unknown
            } else {
                DiffRows::Hunks(
                    diff.hunks()
                        .iter()
                        .map(|hunk| (hunk.left.clone(), hunk.right.clone()))
                        .collect(),
                )
            }
        });
        Self {
            base: Lines::new(base),
            current: Lines::new(current),
            diff,
        }
    }

    /// Where `range` (bytes of `base`) is in `current`, or `None` if the rule
    /// doesn't place it. A range off `base`'s text, or splitting a character,
    /// isn't placed.
    pub fn place(&self, range: Range<usize>) -> Option<Range<usize>> {
        let text = self.base.text;
        if range.start > range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return None;
        }
        let hunks = match &self.diff {
            None => return Some(range),
            Some(DiffRows::Unknown) => return None,
            Some(DiffRows::Hunks(hunks)) => hunks,
        };
        let rows = self.base.window(&range);
        // Rule 1: no change touches the window; the rows before it shift by
        // the rows the changes above it added or removed.
        let mut shift: i64 = 0;
        for (left, right) in hunks {
            if left.start <= rows.end && left.end >= rows.start {
                return None;
            }
            if left.end < rows.start {
                shift += right.len() as i64 - left.len() as i64;
            } else {
                break;
            }
        }
        let image_row = u32::try_from(i64::from(rows.start) + shift).ok()?;
        let window = self.base.bytes(rows.clone());
        let image = self.current.start(image_row);
        // Rule 2: the window's text is unique in both, and in current it is
        // where the diff puts it.
        let needle = &text[window.clone()];
        if self.base.occurrences(needle, window.start) != Some(window.start)
            || self.current.occurrences(needle, image) != Some(image)
        {
            return None;
        }
        let offset = range.start - window.start;
        Some(image + offset..image + offset + range.len())
    }
}

/// The row diff of `base` and `lane` as byte ranges of `base` and the text
/// replacing each. A hunk reaching the end of the text takes the line break
/// before it rather than one after (there is none), so a last row without
/// one is replaced right.
fn lane_hunks(base: &str, lane: &str) -> Vec<(Range<usize>, String)> {
    if base == lane {
        return Vec::new();
    }
    let diff = TextDiff::new(base, lane);
    if diff.too_large() {
        return vec![(0..base.len(), lane.to_owned())];
    }
    let (old, new) = (Lines::new(base), Lines::new(lane));
    diff.hunks()
        .iter()
        .map(|hunk| {
            let at_end = hunk.left.end as usize >= old.rows();
            let bytes = |lines: &Lines, rows: &Range<u32>| {
                if at_end && rows.start > 0 {
                    // Rows past the last are none: from the end.
                    let start = if (rows.start as usize) < lines.rows() {
                        lines.start(rows.start) - 1
                    } else {
                        lines.text.len()
                    };
                    start..lines.text.len()
                } else {
                    lines.start(rows.start)..lines.start(rows.end)
                }
            };
            let old_range = bytes(&old, &hunk.left);
            let new_range = bytes(&new, &hunk.right);
            (old_range, lane[new_range].to_owned())
        })
        .collect()
}

/// A text's rows, split at `\n` as [`TextDiff`] splits an LF text: a text
/// ending in a line break has an empty last row.
struct Lines<'a> {
    text: &'a str,
    /// Each row's first byte.
    starts: Vec<usize>,
    /// Rows by their text (without the line break), for finding a window's
    /// occurrences; made on first use.
    index: std::cell::OnceCell<HashMap<&'a str, Vec<u32>>>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Self {
            text,
            starts,
            index: std::cell::OnceCell::new(),
        }
    }

    fn rows(&self) -> usize {
        self.starts.len()
    }

    /// Row `row`'s first byte; the text's length past the last row.
    fn start(&self, row: u32) -> usize {
        self.starts
            .get(row as usize)
            .copied()
            .unwrap_or(self.text.len())
    }

    /// The row holding byte `at`: the row after a line break for one at a
    /// row's start.
    fn row_of(&self, at: usize) -> u32 {
        (self.starts.partition_point(|&start| start <= at) - 1) as u32
    }

    fn row(&self, row: u32) -> &'a str {
        let start = self.start(row);
        let end = self.start(row + 1);
        let end = if end > start && self.text.as_bytes()[end - 1] == b'\n' {
            end - 1
        } else {
            end
        };
        &self.text[start..end]
    }

    /// The window of `range`: the rows it touches (for an empty range, the
    /// row it is in), plus one on each side where there is one.
    fn window(&self, range: &Range<usize>) -> Range<u32> {
        let first = self.row_of(range.start);
        let last = if range.is_empty() {
            first
        } else {
            self.row_of(range.end - 1)
        };
        first.saturating_sub(1)..(last + 2).min(self.rows() as u32)
    }

    /// Rows `rows` with their line breaks, the last row's (if the text's
    /// last) without.
    fn bytes(&self, rows: Range<u32>) -> Range<usize> {
        self.start(rows.start)..self.start(rows.end)
    }

    /// Where `needle` (a window: whole rows, from a row's start) occurs in
    /// the text if it occurs exactly once, anywhere, overlaps counted; `None`
    /// if it occurs more often or not at all. `hint`, where the caller
    /// expects it, lets a direct search stop at the first other occurrence.
    ///
    /// An occurrence lines up the needle's line breaks with the text's, so
    /// all but its first and last rows are whole rows of the text: those are
    /// found through an index of the rows by text, from the rarest of them,
    /// stopping at the second match. A needle with no such row (at most two
    /// rows, so a window at the end of a text, or of a two-row text) is
    /// searched for directly.
    fn occurrences(&self, needle: &str, hint: usize) -> Option<usize> {
        let parts: Vec<&str> = needle.split('\n').collect();
        // Parts 1..last are rows the text must have whole, with their line
        // breaks; the last part, before the needle's end, only begins a row.
        if parts.len() < 3 {
            return self.occurrences_by_search(needle, hint);
        }
        let whole = &parts[1..parts.len() - 1];
        let index = self.index.get_or_init(|| {
            let mut index: HashMap<&str, Vec<u32>> = HashMap::new();
            // The last row has no line break, so it can't be a whole row.
            for row in 0..self.rows().saturating_sub(1) as u32 {
                index.entry(self.row(row)).or_default().push(row);
            }
            index
        });
        let (anchor, rows) = whole
            .iter()
            .enumerate()
            .map(|(i, part)| (i + 1, index.get(part).map_or(&[][..], Vec::as_slice)))
            .min_by_key(|(_, rows)| rows.len())?;
        let mut found = None;
        for &row in rows {
            let Some(first) = row.checked_sub(anchor as u32) else {
                continue;
            };
            // The needle starts this far before the first row's line break.
            let line_break = self.start(first + 1) - 1;
            let Some(at) = line_break.checked_sub(parts[0].len()) else {
                continue;
            };
            if at + needle.len() <= self.text.len()
                && self.text.as_bytes()[at..at + needle.len()] == *needle.as_bytes()
            {
                if found.is_some() {
                    return None;
                }
                found = Some(at);
            }
        }
        found
    }

    fn occurrences_by_search(&self, needle: &str, hint: usize) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let at_hint = bytes
            .get(hint..hint + needle.len())
            .is_some_and(|there| there == needle.as_bytes());
        let mut found = None;
        let mut from = 0;
        while let Some(at) = self.text.get(from..).and_then(|rest| rest.find(needle)) {
            let at = from + at;
            if found.is_some() || (at_hint && at != hint) {
                return None;
            }
            found = Some(at);
            // Overlapping occurrences count: look again a character on.
            from = at + self.text[at..].chars().next().map_or(1, char::len_utf8);
        }
        found
    }
}

/// `text` with `\r\n` and `\r` as `\n`, as a buffer holds it.
fn lf(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        text.into()
    }
}

/// What a whole-file change needs of the user's file, checked when it is
/// proposed and again when it is accepted (§ New and deleted files). A
/// rename is a deletion and a new file, each with its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilePrecondition {
    /// A new file: the path must not exist, on disk or as a buffer.
    Absent,
    /// A deletion: the user's text must equal the text it was made against
    /// (the revision before, for a worktree lane).
    Text(Arc<str>),
}

/// What [`FilePrecondition::check`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreconditionCheck {
    /// The change can be accepted.
    Holds,
    /// The user's file disagrees: a new file's path exists (add/add), or a
    /// deleted file was edited (delete/modify). The change is stale.
    Stale,
    /// A deletion of a file already gone: accepting changes nothing, and the
    /// deletion is decided.
    AlreadyDone,
}

impl FilePrecondition {
    /// The precondition of a lane's change to one path, `None` for the path's
    /// text before and after: a new file's when it had none, a deletion's
    /// when it has none after. `None` when it is neither (a change to the
    /// text goes through [`lane_project`], and no file either side is no
    /// change).
    pub fn of(before: Option<&str>, after: Option<&str>) -> Option<Self> {
        match (before, after) {
            (None, Some(_)) => Some(Self::Absent),
            (Some(before), None) => Some(Self::Text(before.into())),
            _ => None,
        }
    }

    /// Checks against the file as it is now: `now` is its text (its buffer's,
    /// else the disk's), `None` if the path exists neither on disk nor as a
    /// buffer. A deletion's texts are compared with line endings read as a
    /// buffer reads them, so a buffer's text equals its file's.
    pub fn check(&self, now: Option<&str>) -> PreconditionCheck {
        match (self, now) {
            (Self::Absent, None) => PreconditionCheck::Holds,
            (Self::Absent, Some(_)) => PreconditionCheck::Stale,
            (Self::Text(_), None) => PreconditionCheck::AlreadyDone,
            (Self::Text(expected), Some(now)) if lf(expected) == lf(now) => {
                PreconditionCheck::Holds
            }
            (Self::Text(_), Some(_)) => PreconditionCheck::Stale,
        }
    }
}
