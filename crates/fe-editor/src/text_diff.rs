//! A row diff of two texts, a left (the old text: a file's text in git, say)
//! and a right (the new), for diff views and hunk staging: its hunks, how the
//! two sides line up side by side, where removed rows show inline, moving
//! between hunks, and rebuilding the left text with some hunks applied.
//! GPUI-free.
//!
//! Rows are split as [`fe_text::Text`] splits them: `\r\n`, `\r` and `\n` all
//! end a row, so a text ending in a line break has an empty last row.

use std::ops::Range;

use imara_diff::InternedInput;

use crate::row_changes::{self, rows};

/// A text over this many bytes isn't diffed.
pub const MAX_DIFF_BYTES: usize = row_changes::MAX_DIFF_BYTES;

/// One of the two texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkKind {
    /// Rows only the right has.
    Added,
    /// Rows only the left has.
    Removed,
    /// Left rows the right replaces.
    Modified,
}

/// A run of rows that differ, in rows of each side. An empty range is an
/// insertion point: the row the other side's rows sit above, or the side's
/// row count at its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub left: Range<u32>,
    pub right: Range<u32>,
}

impl Hunk {
    pub fn kind(&self) -> HunkKind {
        if self.left.is_empty() {
            HunkKind::Added
        } else if self.right.is_empty() {
            HunkKind::Removed
        } else {
            HunkKind::Modified
        }
    }

    /// The hunk's rows on `side`.
    pub fn range(&self, side: Side) -> &Range<u32> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

/// The row diff of a left and a right text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextDiff {
    hunks: Vec<Hunk>,
    too_large: bool,
}

impl TextDiff {
    /// Diffs `left` and `right` by row as [`row_changes`] does: with
    /// imara-diff's Histogram when the region between the common prefix and
    /// suffix is small, else its Myers (see `row_changes::row_diff`). Either over [`MAX_DIFF_BYTES`] gives no hunks.
    pub fn new(left: &str, right: &str) -> Self {
        if left.len() > MAX_DIFF_BYTES || right.len() > MAX_DIFF_BYTES {
            return Self {
                hunks: Vec::new(),
                too_large: true,
            };
        }
        let (mut left_scratch, mut right_scratch) = (String::new(), String::new());
        let input = InternedInput::new(
            rows(left, &mut left_scratch),
            rows(right, &mut right_scratch),
        );
        let mut diff = row_changes::row_diff(&input);
        diff.postprocess_lines(&input);
        let hunks = diff
            .hunks()
            .map(|hunk| Hunk {
                left: hunk.before,
                right: hunk.after,
            })
            .collect();
        Self {
            hunks,
            too_large: false,
        }
    }

    /// Whether a side was over [`MAX_DIFF_BYTES`], so nothing was diffed.
    pub fn too_large(&self) -> bool {
        self.too_large
    }

    /// The hunks, sorted, with at least one unchanged row between two on
    /// both sides.
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    /// Side by side, the filler rows `side` shows so the k-th rows after
    /// every hunk line up, as `(row they sit above, rows)`, sorted: for each
    /// hunk the shorter side gets the difference above its range's end.
    pub fn fillers(&self, side: Side) -> Vec<(u32, u32)> {
        self.fillers_iter(side).collect()
    }

    fn fillers_iter(&self, side: Side) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.hunks.iter().filter_map(move |hunk| {
            let (own, other) = match side {
                Side::Left => (&hunk.left, &hunk.right),
                Side::Right => (&hunk.right, &hunk.left),
            };
            let short = other.len().saturating_sub(own.len()) as u32;
            (short > 0).then_some((own.end, short))
        })
    }

    /// The screen row `row` of `side` shows on side by side: the row plus
    /// the fillers above it.
    pub fn screen_row(&self, side: Side, row: u32) -> u32 {
        row + self
            .fillers_iter(side)
            .take_while(|&(above, _)| above <= row)
            .map(|(_, rows)| rows)
            .sum::<u32>()
    }

    /// The row of `side` at `screen` row side by side, and whether `screen`
    /// is a filler row; in one, the row below the fillers.
    pub fn row_at_screen(&self, side: Side, screen: u32) -> (u32, bool) {
        let mut fillers_above = 0;
        for (above, rows) in self.fillers_iter(side) {
            let start = above + fillers_above;
            if screen < start {
                break;
            }
            if screen < start + rows {
                return (above, true);
            }
            fillers_above += rows;
        }
        (screen - fillers_above, false)
    }

    /// Inline, the removed rows to show: each hunk with left rows shows them
    /// above right row `hunk.right.start`, as `(right row, left rows)`.
    pub fn removed_above(&self) -> Vec<(u32, Range<u32>)> {
        self.hunks
            .iter()
            .filter(|hunk| !hunk.left.is_empty())
            .map(|hunk| (hunk.right.start, hunk.left.clone()))
            .collect()
    }

    /// The index of the first hunk starting after `row` on `side`; `None`
    /// past the last one (callers wrap).
    pub fn next_hunk(&self, side: Side, row: u32) -> Option<usize> {
        let next = self
            .hunks
            .partition_point(|hunk| hunk.range(side).start <= row);
        (next < self.hunks.len()).then_some(next)
    }

    /// The index of the last hunk starting before `row` on `side`; `None`
    /// before the first one (callers wrap).
    pub fn prev_hunk(&self, side: Side, row: u32) -> Option<usize> {
        self.hunks
            .partition_point(|hunk| hunk.range(side).start < row)
            .checked_sub(1)
    }

    /// The index of the hunk holding `row` on `side`, or of the one whose
    /// range there is empty with `row` its insertion row.
    pub fn hunk_at(&self, side: Side, row: u32) -> Option<usize> {
        let at = self
            .hunks
            .partition_point(|hunk| hunk.range(side).start <= row)
            .checked_sub(1)?;
        let range = self.hunks[at].range(side);
        (range.contains(&row) || range.start == row).then_some(at)
    }

    /// `left` with the left rows of the hunks at `chosen` (indexes into
    /// [`Self::hunks`], in any order; others are ignored) replaced by their
    /// right rows: what staging those hunks writes. `left` and `right` are
    /// the texts this diff was made from. Kept rows keep `left`'s line
    /// endings and right rows take its first one (`\n` without one), but
    /// for a bare `\r` that would join the next empty row's `\n` into one
    /// `\r\n`. When a chosen hunk reaches either side's last row, the text
    /// ends in a line break if and only if `right` does, dropping every
    /// trailing empty row when it doesn't.
    pub fn apply(&self, left: &str, right: &str, chosen: &[usize]) -> String {
        let mut mask = vec![false; self.hunks.len()];
        for &index in chosen {
            if let Some(chosen) = mask.get_mut(index) {
                *chosen = true;
            }
        }
        self.rebuild(left, right, |index, _, left_rows, right_rows, out| {
            if mask[index] {
                out.extend(right_rows.iter().map(|&(row, _)| (row, None)));
            } else {
                out.extend(left_rows.iter().map(|&(row, ending)| kept(row, ending)));
            }
            mask[index]
        })
    }

    /// `left` with the right rows in `chosen` (rows of `right`, a visual
    /// line range say) staged. In each hunk the i-th right row pairs with
    /// the i-th left row: a chosen right row replaces its pair, or, past the
    /// left rows, goes after the hunk's last left row. Left rows past the
    /// right rows go only when `chosen` holds the hunk's last right row. A
    /// [`HunkKind::Removed`] hunk goes when `chosen` holds its insertion row
    /// or the row above it. Line endings as for [`Self::apply`], a hunk
    /// counting as chosen when any of its rows is.
    pub fn apply_rows(&self, left: &str, right: &str, chosen: Range<u32>) -> String {
        self.rebuild(left, right, |_, hunk, left_rows, right_rows, out| {
            let picked = |row: u32| chosen.contains(&row);
            if hunk.kind() == HunkKind::Removed {
                let at = hunk.right.start;
                let removed = picked(at) || at.checked_sub(1).is_some_and(picked);
                if !removed {
                    out.extend(left_rows.iter().map(|&(row, ending)| kept(row, ending)));
                }
                return removed;
            }
            let last_picked = picked(hunk.right.end - 1);
            for (i, &(row, ending)) in left_rows.iter().enumerate() {
                match right_rows.get(i) {
                    Some(&(new, _)) if picked(hunk.right.start + i as u32) => out.push((new, None)),
                    Some(_) => out.push(kept(row, ending)),
                    None if last_picked => {}
                    None => out.push(kept(row, ending)),
                }
            }
            for (i, &(new, _)) in right_rows.iter().enumerate().skip(left_rows.len()) {
                if picked(hunk.right.start + i as u32) {
                    out.push((new, None));
                }
            }
            chosen.start.max(hunk.right.start) < chosen.end.min(hunk.right.end)
        })
    }

    /// `left` rebuilt row by row: rows outside hunks kept, and each hunk's
    /// output rows pushed by `hunk_rows(index, hunk, its left rows, its
    /// right rows, out)`, which returns whether it applied any of the hunk.
    /// A row is its text and, for a kept row that had one, its ending;
    /// others take `left`'s first ending, and the last row takes none.
    fn rebuild<'a>(
        &self,
        left: &'a str,
        right: &'a str,
        mut hunk_rows: impl FnMut(
            usize,
            &Hunk,
            &[SplitRow<'a>],
            &[SplitRow<'a>],
            &mut Vec<OutRow<'a>>,
        ) -> bool,
    ) -> String {
        let (left_rows, right_rows) = (split_rows(left), split_rows(right));
        let first_ending = left_rows
            .first()
            .map(|&(_, ending)| ending)
            .filter(|ending| !ending.is_empty())
            .unwrap_or("\n");
        let mut out = Vec::with_capacity(left_rows.len());
        let mut next = 0;
        let mut at_end = false;
        for (index, hunk) in self.hunks.iter().enumerate() {
            let before = clamp(&left_rows, next..hunk.left.start);
            out.extend(before.iter().map(|&(row, ending)| kept(row, ending)));
            let applied = hunk_rows(
                index,
                hunk,
                clamp(&left_rows, hunk.left.clone()),
                clamp(&right_rows, hunk.right.clone()),
                &mut out,
            );
            at_end |= applied
                && (hunk.left.end as usize >= left_rows.len()
                    || hunk.right.end as usize >= right_rows.len());
            next = hunk.left.end;
        }
        let after = clamp(&left_rows, next..left_rows.len() as u32);
        out.extend(after.iter().map(|&(row, ending)| kept(row, ending)));

        // The text ends in a line break when it has more than one row and
        // the last is empty.
        if at_end {
            let breaks =
                |out: &Vec<OutRow>| out.len() > 1 && out.last().is_some_and(|row| row.0.is_empty());
            if right.ends_with(['\n', '\r']) {
                while !breaks(&out) {
                    out.push(("", None));
                }
            } else {
                while breaks(&out) {
                    out.pop();
                }
            }
        }

        // Each row but the last is followed by its ending.
        let mut endings: Vec<(&str, bool)> = out
            .iter()
            .take(out.len().saturating_sub(1))
            .map(|&(_, ending)| (ending.unwrap_or(first_ending), ending.is_some()))
            .collect();
        // A bare `\r` then an empty row ending in `\n` would read back as one
        // `\r\n`, losing the empty row: change a made-up ending rather than
        // a kept one when there's the choice.
        for i in 1..endings.len() {
            if endings[i - 1].0 == "\r" && out[i].0.is_empty() && endings[i].0 == "\n" {
                if endings[i - 1].1 && !endings[i].1 {
                    endings[i].0 = "\r";
                } else {
                    endings[i - 1].0 = "\r\n";
                }
            }
        }
        let mut text = String::with_capacity(left.len());
        for (i, (row, _)) in out.iter().enumerate() {
            text.push_str(row);
            if let Some((ending, _)) = endings.get(i) {
                text.push_str(ending);
            }
        }
        text
    }
}

/// A row's text and its line ending, empty on the last row.
type SplitRow<'a> = (&'a str, &'a str);

/// A rebuilt row's text and the ending it keeps, if any.
type OutRow<'a> = (&'a str, Option<&'a str>);

fn kept<'a>(row: &'a str, ending: &'a str) -> OutRow<'a> {
    (row, (!ending.is_empty()).then_some(ending))
}

/// `rows[range]`, cut to the rows there are, so texts other than the diffed
/// ones can't panic.
fn clamp<'r, 'a>(rows: &'r [SplitRow<'a>], range: Range<u32>) -> &'r [SplitRow<'a>] {
    let end = (range.end as usize).min(rows.len());
    &rows[(range.start as usize).min(end)..end]
}

/// `text`'s rows with their endings, split as [`rows`] splits.
fn split_rows(text: &str) -> Vec<SplitRow<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        let len = match bytes[i] {
            b'\n' => 1,
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => 2,
            b'\r' => 1,
            _ => {
                i += 1;
                continue;
            }
        };
        out.push((&text[start..i], &text[i..i + len]));
        i += len;
        start = i;
    }
    out.push((&text[start..], ""));
    out
}
