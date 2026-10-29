//! The changed characters inside a [`TextDiff`]'s modified rows, for diff
//! views to darken within a row's tint: each [`HunkKind::Modified`] hunk
//! pairs its i-th left row with its i-th right row, and a pair is diffed by
//! word tokens (runs of word characters, runs of whitespace, and single
//! other graphemes) with imara-diff. Rows of added and removed hunks, rows
//! past the shorter side of a modified hunk, and pairs over the size caps
//! report no spans: callers tint those rows whole. GPUI-free.

use std::cell::OnceCell;
use std::ops::Range;

use imara_diff::InternedInput;
use unicode_segmentation::UnicodeSegmentation;

use crate::CharClass;
use crate::row_changes;
use crate::text_diff::{Hunk, HunkKind, Side, TextDiff};

/// A row pair with either row over this many bytes isn't diffed.
pub const MAX_INLINE_BYTES: usize = 1_000;

/// A modified hunk with more rows than this on either side isn't diffed.
pub const MAX_PAIRED_ROWS: usize = 200;

impl TextDiff {
    /// The changes inside this diff's modified rows, `left` and `right`
    /// being the texts it was made from. Nothing is diffed until asked:
    /// see [`InlineDiff::changed_spans`].
    pub fn inline<'a>(&'a self, left: &'a str, right: &'a str) -> InlineDiff<'a> {
        InlineDiff {
            diff: self,
            texts: [left, right],
            rows: Default::default(),
            hunks: (0..self.hunks().len()).map(|_| OnceCell::new()).collect(),
        }
    }
}

/// A [`TextDiff`]'s changes inside its modified rows, worked out a hunk at a
/// time when one of its rows is first asked for and kept from then on.
pub struct InlineDiff<'a> {
    diff: &'a TextDiff,
    texts: [&'a str; 2],
    /// Each side's rows as byte ranges of its text, split when first needed.
    rows: [OnceCell<Vec<Range<usize>>>; 2],
    /// Per hunk, its spans once worked out.
    hunks: Vec<OnceCell<HunkSpans>>,
}

/// Per side, the spans of each of a hunk's paired rows, in order.
type HunkSpans = [Vec<Vec<Range<usize>>>; 2];

impl InlineDiff<'_> {
    /// The changed bytes of `row` on `side`, as sorted, disjoint byte
    /// ranges within the row's text (its line ending left out), each
    /// starting and ending on a grapheme boundary. Empty for a row outside
    /// a modified hunk, past its pairs, or over the caps
    /// ([`MAX_INLINE_BYTES`], [`MAX_PAIRED_ROWS`]): those are tinted whole.
    pub fn changed_spans(&self, side: Side, row: u32) -> &[Range<usize>] {
        let Some(index) = self.diff.hunk_at(side, row) else {
            return &[];
        };
        let hunk = &self.diff.hunks()[index];
        let rows = hunk.range(side);
        if hunk.kind() != HunkKind::Modified || !rows.contains(&row) {
            return &[];
        }
        let spans = self.hunks[index].get_or_init(|| self.hunk_spans(hunk));
        spans[side as usize]
            .get((row - rows.start) as usize)
            .map_or(&[], Vec::as_slice)
    }

    fn hunk_spans(&self, hunk: &Hunk) -> HunkSpans {
        let (left, right) = (hunk.left.len(), hunk.right.len());
        let mut spans = HunkSpans::default();
        if left.max(right) > MAX_PAIRED_ROWS {
            return spans;
        }
        let mut input = InternedInput::default();
        for i in 0..left.min(right) as u32 {
            let [left, right] = pair_spans(
                &mut input,
                self.row(Side::Left, hunk.left.start + i),
                self.row(Side::Right, hunk.right.start + i),
            );
            spans[0].push(left);
            spans[1].push(right);
        }
        spans
    }

    /// The text of `row` on `side`, empty past its last row (when the texts
    /// aren't the ones diffed).
    fn row(&self, side: Side, row: u32) -> &str {
        let text = self.texts[side as usize];
        let rows = self.rows[side as usize].get_or_init(|| split_rows(text));
        rows.get(row as usize)
            .map_or("", |range| &text[range.clone()])
    }
}

/// The changed bytes of a left and a right row, as for
/// [`InlineDiff::changed_spans`]. Diffed by [`row_changes::row_diff`] on
/// word tokens, so its work bound holds here too.
fn pair_spans<'a>(
    input: &mut InternedInput<&'a str>,
    left: &'a str,
    right: &'a str,
) -> [Vec<Range<usize>>; 2] {
    let mut spans: [Vec<Range<usize>>; 2] = Default::default();
    if left.len() > MAX_INLINE_BYTES || right.len() > MAX_INLINE_BYTES {
        return spans;
    }
    let tokens = [tokens(left), tokens(right)];
    input.clear();
    input.update_before(tokens[0].iter().map(|token| &left[token.clone()]));
    input.update_after(tokens[1].iter().map(|token| &right[token.clone()]));
    let mut diff = row_changes::row_diff(input);
    diff.postprocess_no_heuristic(input);
    for hunk in diff.hunks() {
        push_span(&mut spans[0], &tokens[0], hunk.before);
        push_span(&mut spans[1], &tokens[1], hunk.after);
    }
    spans
}

/// Adds the bytes of `tokens[changed]` to `spans`, joining the last span
/// when they meet it.
fn push_span(spans: &mut Vec<Range<usize>>, tokens: &[Range<usize>], changed: Range<u32>) {
    if changed.is_empty() {
        return;
    }
    let (start, end) = (
        tokens[changed.start as usize].start,
        tokens[changed.end as usize - 1].end,
    );
    match spans.last_mut() {
        Some(last) if last.end == start => last.end = end,
        _ => spans.push(start..end),
    }
}

/// `row`'s word tokens as byte ranges: runs of graphemes that start with a
/// word character, runs that start with whitespace, and each other grapheme
/// alone.
fn tokens(row: &str) -> Vec<Range<usize>> {
    let mut tokens: Vec<Range<usize>> = Vec::new();
    let mut last = None;
    for (start, grapheme) in row.grapheme_indices(true) {
        let class = grapheme.chars().next().map(CharClass::of);
        let end = start + grapheme.len();
        match tokens.last_mut() {
            Some(token) if class == last && class != Some(CharClass::Punctuation) => {
                token.end = end
            }
            _ => tokens.push(start..end),
        }
        last = class;
    }
    tokens
}

/// `text`'s rows, without their endings, split as [`TextDiff`] splits:
/// `\r\n`, `\r` and `\n` each end a row.
fn split_rows(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut rows = Vec::new();
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
        rows.push(start..i);
        i += len;
        start = i;
    }
    rows.push(start..text.len());
    rows
}

#[cfg(test)]
#[path = "inline_diff_tests.rs"]
mod inline_diff_tests;
