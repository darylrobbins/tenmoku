// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, files
// crates/base/src/input/editor/display_map/{text_wrapper,wrap_map}.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: GPUI-free (wrapping measures through `CharWidth`),
// lines are read through `fe_text::TextSource`, items store only their wrap
// points, an unwrapped map keeps no tree at all, and a new width re-wraps
// rows in slices rather than all at once.

//! Soft wrapping: buffer rows ↔ wrap rows.
//!
//! Each wrapped buffer row is one item in a [`SumTree`] holding the byte
//! offsets where its continuation rows start, and a run of unwrapped rows is
//! one item, so mapping between buffer rows and wrap rows is a seek, and an
//! edit re-wraps only the rows it touched.
//!
//! A new width, tab size or font doesn't re-wrap anything by itself: every
//! row becomes pending and keeps showing its previous wrap until
//! [`WrapMap::wrap_pending`] gets to it.

use std::{borrow::Cow, ops::Range};

use fe_text::TextSource;
use sum_tree::{Bias, ContextLessSummary, Dimension, Dimensions, Item, SumTree};

use crate::CharWidth;

#[derive(Clone, Debug)]
enum LineItem {
    /// One buffer row: byte offsets within it where continuation rows
    /// start, ascending.
    Wrapped(Box<[u32]>),
    /// This many buffer rows (at least one), none of them wrapped.
    Unwrapped(u32),
}

#[derive(Clone, Debug, Default)]
struct LineSummary {
    buffer_rows: usize,
    wrap_rows: usize,
}

impl ContextLessSummary for LineSummary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, other: &Self) {
        self.buffer_rows += other.buffer_rows;
        self.wrap_rows += other.wrap_rows;
    }
}

impl Item for LineItem {
    type Summary = LineSummary;

    fn summary(&self, (): ()) -> LineSummary {
        match self {
            Self::Wrapped(breaks) => LineSummary {
                buffer_rows: 1,
                wrap_rows: breaks.len() + 1,
            },
            &Self::Unwrapped(rows) => LineSummary {
                buffer_rows: rows as usize,
                wrap_rows: rows as usize,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct BufferRows(usize);

impl Dimension<'_, LineSummary> for BufferRows {
    fn zero((): ()) -> Self {
        Self(0)
    }

    fn add_summary(&mut self, summary: &LineSummary, (): ()) {
        self.0 += summary.buffer_rows;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct WrapRows(usize);

impl Dimension<'_, LineSummary> for WrapRows {
    fn zero((): ()) -> Self {
        Self(0)
    }

    fn add_summary(&mut self, summary: &LineSummary, (): ()) {
        self.0 += summary.wrap_rows;
    }
}

/// Where one wrap row is: its buffer row, which segment of it, and the
/// segment's byte range within the buffer row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WrapSegment {
    pub(crate) buffer_row: u32,
    pub(crate) index: usize,
    pub(crate) segments: usize,
    pub(crate) range: Range<usize>,
}

pub(crate) struct WrapMap {
    /// `None` doesn't wrap: every buffer row is one wrap row, `lines` is
    /// empty and nothing is pending.
    width: Option<f32>,
    tab_size: usize,
    line_count: usize,
    lines: SumTree<LineItem>,
    pending: Pending,
    ascii: AsciiWidths,
}

impl WrapMap {
    pub(crate) fn new(text: &dyn TextSource, tab_size: usize) -> Self {
        Self {
            width: None,
            tab_size: tab_size.max(1),
            line_count: text.row_count() as usize,
            lines: SumTree::new(()),
            pending: Pending::default(),
            ascii: AsciiWidths::default(),
        }
    }

    pub(crate) fn width(&self) -> Option<f32> {
        self.width
    }

    /// Wraps at `width` from now on, measuring with `widths`. Re-wraps
    /// nothing yet: every row is pending, showing its previous wrap (or
    /// none) until [`WrapMap::wrap_pending`] reaches it.
    pub(crate) fn rebuild(
        &mut self,
        text: &dyn TextSource,
        width: Option<f32>,
        tab_size: usize,
        widths: &mut dyn CharWidth,
    ) {
        self.tab_size = tab_size.max(1);
        debug_assert_eq!(self.line_count, text.row_count() as usize);
        match width {
            None => {
                self.lines = SumTree::new(());
                self.pending = Pending::default();
            }
            Some(width) => {
                self.ascii = AsciiWidths::new(width, widths);
                if self.width.is_none() {
                    self.lines = unwrapped(self.line_count);
                }
                self.pending = Pending::all(self.line_count);
            }
        }
        self.width = width;
    }

    /// Follows `text`, which isn't an edit of the text so far: every row
    /// shows unwrapped and, when wrapping, is pending.
    pub(crate) fn reset(&mut self, text: &dyn TextSource) {
        self.line_count = text.row_count() as usize;
        if self.width.is_some() {
            self.lines = unwrapped(self.line_count);
            self.pending = Pending::all(self.line_count);
        }
    }

    /// Replaces `old_rows` rows starting at `start` with `new_rows` rows of
    /// `text`, which is the text after the edit, wrapping them.
    pub(crate) fn splice(
        &mut self,
        text: &dyn TextSource,
        start: usize,
        old_rows: usize,
        new_rows: usize,
        widths: &mut dyn CharWidth,
    ) {
        self.line_count = self.line_count + new_rows - old_rows;
        let Some(width) = self.width else {
            return;
        };
        let mut wrapper = Wrapper::new(width, self.tab_size, &self.ascii, widths);
        let mut rows = RowReader::new(text, start as u32..(start + new_rows) as u32);
        let new_lines = (0..new_rows).map(|_| wrapper.wrap_next(&mut rows).0);
        replace_rows(&mut self.lines, start..start + old_rows, new_lines);
        self.pending
            .splice(start as u32, old_rows as u32, new_rows as u32);
        debug_assert_eq!(self.lines.summary().buffer_rows, self.line_count);
    }

    pub(crate) fn is_pending(&self) -> bool {
        !self.pending.0.is_empty()
    }

    /// Wraps pending rows of `text`, those from `around` onward first, then
    /// those before it, nearest first, until it has wrapped `budget` bytes
    /// of rows, newlines included (at least one row). Returns whether rows
    /// are still pending.
    pub(crate) fn wrap_pending(
        &mut self,
        text: &dyn TextSource,
        around: u32,
        budget: usize,
        widths: &mut dyn CharWidth,
    ) -> bool {
        let Some(width) = self.width else {
            return false;
        };
        let mut wrapper = Wrapper::new(width, self.tab_size, &self.ascii, widths);
        let mut spent = 0;
        let mut new_lines = Vec::new();
        while spent < budget || spent == 0 {
            let ix = self.pending.0.partition_point(|rows| rows.end <= around);
            new_lines.clear();
            let rows = if let Some(rows) = self.pending.0.get(ix) {
                let start = rows.start.max(around);
                let mut end = start;
                let mut reader = RowReader::new(text, start..rows.end);
                while end < rows.end && (spent < budget || spent == 0) {
                    let (line, len) = wrapper.wrap_next(&mut reader);
                    spent += len + 1;
                    new_lines.push(line);
                    end += 1;
                }
                start..end
            } else if let Some(rows) = self.pending.0.last() {
                let mut start = rows.end;
                while start > rows.start && (spent < budget || spent == 0) {
                    start -= 1;
                    let (line, len) = wrapper.wrap_row(text, start);
                    spent += len + 1;
                    new_lines.push(line);
                }
                new_lines.reverse();
                start..rows.end
            } else {
                break;
            };
            replace_rows(
                &mut self.lines,
                rows.start as usize..rows.end as usize,
                new_lines.drain(..),
            );
            self.pending
                .splice(rows.start, rows.len() as u32, rows.len() as u32);
        }
        self.is_pending()
    }

    pub(crate) fn row_count(&self) -> usize {
        match self.width {
            None => self.line_count,
            Some(_) => self.lines.summary().wrap_rows,
        }
    }

    /// The first wrap row of `buffer_row`, or the row count past the end.
    pub(crate) fn first_row(&self, buffer_row: usize) -> usize {
        if self.width.is_none() {
            return buffer_row.min(self.line_count);
        }
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(());
        cursor.seek(&BufferRows(buffer_row), Bias::Right);
        let (start, first) = (cursor.start().0.0, cursor.start().1.0);
        match cursor.item() {
            Some(LineItem::Unwrapped(_)) => first + buffer_row - start,
            _ => first,
        }
    }

    /// The wrap row showing byte `column` of `buffer_row`. A wrap point
    /// starts the row after it.
    pub(crate) fn row_for(&self, buffer_row: usize, column: usize) -> usize {
        if self.width.is_none() {
            return buffer_row;
        }
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(());
        cursor.seek(&BufferRows(buffer_row), Bias::Right);
        let (start, first) = (cursor.start().0.0, cursor.start().1.0);
        match cursor.item() {
            Some(LineItem::Wrapped(breaks)) => {
                first + breaks.partition_point(|&at| at as usize <= column)
            }
            Some(LineItem::Unwrapped(_)) => first + buffer_row - start,
            None => first,
        }
    }

    /// Where wrap row `row` (clamped to the last) is.
    pub(crate) fn segment(&self, text: &dyn TextSource, row: usize) -> WrapSegment {
        let row = row.min(self.row_count().saturating_sub(1));
        let (buffer_row, breaks, index) = if self.width.is_none() {
            (row, &[][..], 0)
        } else {
            let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(());
            cursor.seek(&WrapRows(row), Bias::Right);
            let (first, start) = (cursor.start().0.0, cursor.start().1.0);
            match cursor.item().expect("row is in range") {
                LineItem::Wrapped(breaks) => (start, &breaks[..], row - first),
                LineItem::Unwrapped(_) => (start + row - first, &[][..], 0),
            }
        };
        let len = text.row_len(buffer_row as u32) as usize;
        let start = index.checked_sub(1).map_or(0, |ix| breaks[ix] as usize);
        let end = breaks.get(index).map_or(len, |&at| at as usize);
        WrapSegment {
            buffer_row: buffer_row as u32,
            index,
            segments: breaks.len() + 1,
            range: start..end,
        }
    }

    #[cfg(test)]
    pub(crate) fn item_count(&self) -> usize {
        self.lines.iter().count()
    }
}

/// `rows` buffer rows, unwrapped.
fn unwrapped(rows: usize) -> SumTree<LineItem> {
    let mut lines = SumTree::new(());
    if rows > 0 {
        lines.push(LineItem::Unwrapped(rows as u32), ());
    }
    lines
}

/// Replaces buffer rows `old` of `lines` with `new`, splitting the unwrapped
/// runs around them.
fn replace_rows(
    lines: &mut SumTree<LineItem>,
    old: Range<usize>,
    new: impl IntoIterator<Item = LineItem>,
) {
    let mut cursor = lines.cursor::<BufferRows>(());
    let mut result = cursor.slice(&BufferRows(old.start), Bias::Right);
    let before = old.start - cursor.start().0;
    if before > 0 {
        result.push(LineItem::Unwrapped(before as u32), ());
    }
    result.extend(new, ());
    cursor.seek_forward(&BufferRows(old.end), Bias::Right);
    if cursor.item().is_some() && cursor.start().0 < old.end {
        let after = cursor.end().0 - old.end;
        result.push(LineItem::Unwrapped(after as u32), ());
        cursor.next();
    }
    result.append(cursor.suffix(), ());
    drop(cursor);
    *lines = result;
}

/// Buffer rows still to wrap at the current width: sorted, disjoint and
/// non-adjacent.
#[derive(Debug, Default)]
struct Pending(Vec<Range<u32>>);

impl Pending {
    fn all(rows: usize) -> Self {
        Self((rows > 0).then_some(0..rows as u32).into_iter().collect())
    }

    /// Follows rows `start..start + old_rows` being replaced by `new_rows`
    /// wrapped ones; later rows shift.
    fn splice(&mut self, start: u32, old_rows: u32, new_rows: u32) {
        let old_end = start + old_rows;
        let shift = |row: u32| row + new_rows - old_rows;
        let mut ranges: Vec<Range<u32>> = Vec::with_capacity(self.0.len() + 1);
        for rows in self.0.drain(..) {
            let before = rows.start..rows.end.min(start);
            let after =
                (rows.end > old_end).then(|| shift(rows.start.max(old_end))..shift(rows.end));
            for piece in [Some(before), after].into_iter().flatten() {
                if piece.is_empty() {
                    continue;
                }
                match ranges.last_mut() {
                    Some(last) if last.end == piece.start => last.end = piece.end,
                    _ => ranges.push(piece),
                }
            }
        }
        self.0 = ranges;
    }
}

/// The widths of ASCII chars, measured once per [`WrapMap::rebuild`].
#[derive(Default)]
struct AsciiWidths {
    widths: Option<Box<[f32; 128]>>,
    /// Rows of at most this many ASCII chars, none of them a tab, fit the
    /// wrap width without measuring.
    fits: usize,
}

impl AsciiWidths {
    /// Caps the work of finding `fits` for huge widths.
    const MAX_FITS: usize = u16::MAX as usize;

    fn new(width: f32, widths: &mut dyn CharWidth) -> Self {
        let mut table = [0.; 128];
        for (ch, slot) in (0..128u8).map(char::from).zip(&mut table) {
            *slot = widths.char_width(ch);
        }
        // Run `break_points`' check on a row of the widest char. Float sums
        // only grow with their terms, so no row of narrower chars breaks
        // where this one doesn't.
        let widest = table.iter().copied().fold(0., f32::max);
        let mut fits = 1;
        let mut x = widest;
        while fits < Self::MAX_FITS && x + widest <= width {
            x += widest;
            fits += 1;
        }
        Self {
            widths: Some(Box::new(table)),
            fits,
        }
    }

    fn get(&self, ch: char) -> Option<f32> {
        self.widths.as_ref()?.get(ch as usize).copied()
    }
}

/// Finds wrap points, with ASCII widths from the cache.
struct Wrapper<'a> {
    width: f32,
    tab_size: usize,
    ascii: &'a AsciiWidths,
    widths: &'a mut dyn CharWidth,
    line: String,
}

impl<'a> Wrapper<'a> {
    fn new(
        width: f32,
        tab_size: usize,
        ascii: &'a AsciiWidths,
        widths: &'a mut dyn CharWidth,
    ) -> Self {
        Self {
            width,
            tab_size,
            ascii,
            widths,
            line: String::new(),
        }
    }

    /// Wraps `row`, returning its length too.
    fn wrap_row(&mut self, text: &dyn TextSource, row: u32) -> (LineItem, usize) {
        self.wrap_next(&mut RowReader::new(text, row..row + 1))
    }

    /// Wraps the next row `rows` reads, returning its length too.
    fn wrap_next(&mut self, rows: &mut RowReader) -> (LineItem, usize) {
        let mut line = std::mem::take(&mut self.line);
        let len = rows.read(&mut line);
        let item = if len <= self.ascii.fits && line.bytes().all(|b| b.is_ascii() && b != b'\t') {
            LineItem::Wrapped(Box::default())
        } else {
            let (ascii, widths) = (self.ascii, &mut *self.widths);
            LineItem::Wrapped(break_points(&line, self.width, self.tab_size, |ch| {
                ascii.get(ch).unwrap_or_else(|| widths.char_width(ch))
            }))
        };
        self.line = line;
        (item, len)
    }
}

/// Reads rows of a text in order, in one pass over it from the first row's
/// start. A run of rows costs two row lookups however long it is, where
/// [`TextSource::row_start`] and [`TextSource::row_len`] for each would take
/// three lookups per row, which on a rope is a tenth of wrapping a row.
struct RowReader<'a> {
    text: &'a dyn TextSource,
    /// The next row.
    row: u32,
    chunks: Box<dyn Iterator<Item = Cow<'a, str>> + 'a>,
    chunk: Cow<'a, str>,
    /// Where the next row starts in `chunk`.
    at: usize,
}

impl<'a> RowReader<'a> {
    /// Reads `rows` of `text`.
    fn new(text: &'a dyn TextSource, rows: Range<u32>) -> Self {
        let chunks: Box<dyn Iterator<Item = Cow<'a, str>>> = if rows.is_empty() {
            Box::new(std::iter::empty())
        } else {
            let start = text.row_start(rows.start);
            let end = if rows.end < text.row_count() {
                text.row_start(rows.end)
            } else {
                text.len()
            };
            text.chunks_in_range(start..end)
        };
        Self {
            text,
            row: rows.start,
            chunks,
            chunk: Cow::Borrowed(""),
            at: 0,
        }
    }

    /// Reads the next row into `line`, without its line break, and returns
    /// its length.
    fn read(&mut self, line: &mut String) -> usize {
        line.clear();
        loop {
            let rest = &self.chunk[self.at..];
            if let Some(ix) = rest.find('\n') {
                line.push_str(&rest[..ix]);
                self.at += ix + 1;
                break;
            }
            line.push_str(rest);
            match self.chunks.next() {
                Some(chunk) => (self.chunk, self.at) = (chunk, 0),
                None => {
                    self.at = self.chunk.len();
                    break;
                }
            }
        }
        // The line break, one byte or two, reads as `\n` from a `Text` or a
        // `FileText`. Read any other way, the row's length says where it
        // ends.
        let len = if line.ends_with('\r') {
            self.text.row_len(self.row) as usize
        } else {
            line.len()
        };
        line.truncate(len);
        debug_assert_eq!(len, self.text.row_len(self.row) as usize);
        self.row += 1;
        len
    }
}

/// Breaks `line` into rows no wider than `width`, after the whitespace
/// before a word where possible, otherwise mid-word. Every row keeps at
/// least one char, and whitespace may hang past the edge. Tab stops count
/// from the start of each row, as the rows are laid out.
fn break_points(
    line: &str,
    width: f32,
    tab_size: usize,
    mut char_width: impl FnMut(char) -> f32,
) -> Box<[u32]> {
    let space = char_width(' ');
    let mut breaks = Vec::new();
    let mut row_start = 0;
    'rows: loop {
        let mut x = 0.;
        let mut column = 0;
        let mut word_start = None;
        let mut after_space = false;
        for (ix, ch) in line[row_start..].char_indices() {
            let is_space = ch.is_whitespace();
            if after_space && !is_space {
                word_start = Some(ix);
            }
            let (ch_width, columns) = if ch == '\t' {
                let columns = tab_size - column % tab_size;
                (space * columns as f32, columns)
            } else {
                (char_width(ch), 1)
            };
            if ix > 0 && !is_space && x + ch_width > width {
                row_start += word_start.unwrap_or(ix);
                breaks.push(row_start as u32);
                continue 'rows;
            }
            x += ch_width;
            column += columns;
            after_space = is_space;
        }
        break;
    }
    breaks.into()
}

#[cfg(test)]
mod tests {
    use fe_text::Text;
    use proptest::prelude::*;

    use super::*;
    use crate::Monospace;

    fn wrap_all(map: &mut WrapMap, text: &Text) {
        assert!(!map.wrap_pending(text, 0, usize::MAX, &mut Monospace));
    }

    fn wrapped(text: &str, width: f32) -> Vec<Vec<String>> {
        let text = Text::new(text);
        let mut map = WrapMap::new(&text, 4);
        map.rebuild(&text, Some(width), 4, &mut Monospace);
        wrap_all(&mut map, &text);
        rows(&map, &text)
    }

    /// The text of each wrap row, grouped by buffer row.
    fn rows(map: &WrapMap, text: &Text) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        for row in 0..map.row_count() {
            let segment = map.segment(text, row);
            let start = text.row_start(segment.buffer_row);
            let shown =
                text.slice_to_string(start + segment.range.start..start + segment.range.end);
            if segment.index == 0 {
                out.push(Vec::new());
            }
            out.last_mut().unwrap().push(shown);
        }
        out
    }

    #[test]
    fn wraps_at_words_then_mid_word() {
        assert_eq!(
            wrapped("one two three\nabcdefghij\n\nx", 6.),
            [
                vec!["one ", "two ", "three"],
                vec!["abcdef", "ghij"],
                vec![""],
                vec!["x"],
            ]
        );
        // Spaces hang past the edge rather than starting a row.
        assert_eq!(wrapped("ab      cd", 4.), [vec!["ab      ", "cd"]]);
        // Tabs count from the row start: `\t` is 4 wide at column 0.
        assert_eq!(wrapped("abc\tdef", 5.), [vec!["abc\t", "def"]]);
        // Each row keeps a char even if it's wider than the wrap width.
        assert_eq!(wrapped("abc", 0.5), [vec!["a", "b", "c"]]);
    }

    /// ASCII chars one to three `scale`s wide; others four.
    struct Uneven {
        scale: f32,
    }

    impl CharWidth for Uneven {
        fn char_width(&mut self, ch: char) -> f32 {
            let units = if ch.is_ascii() { ch as u32 % 3 + 1 } else { 4 };
            units as f32 * self.scale
        }
    }

    proptest! {
        /// Rows tile their buffer row, fit the width, and the splice after an
        /// edit matches wrapping from scratch.
        #[test]
        fn rows_tile_and_splices_match_rebuilds(
            text in "[ab \t\né]{0,60}",
            width in 1usize..12,
            edit in (0usize..60, 0usize..10, "[ab \n]{0,8}"),
        ) {
            let width = width as f32;
            let before = Text::new(&text);
            let mut map = WrapMap::new(&before, 4);
            map.rebuild(&before, Some(width), 4, &mut Monospace);
            wrap_all(&mut map, &before);
            for row in 0..before.row_count() {
                let first = map.first_row(row as usize);
                let segment = map.segment(&before, first);
                prop_assert_eq!(segment.buffer_row, row);
                prop_assert_eq!(segment.index, 0);
                let mut end = 0;
                for ix in 0..segment.segments {
                    let segment = map.segment(&before, first + ix);
                    prop_assert_eq!(segment.range.start, end);
                    prop_assert!(segment.range.end > end || segment.segments == 1);
                    for column in segment.range.clone() {
                        prop_assert_eq!(map.row_for(row as usize, column), first + ix);
                    }
                    end = segment.range.end;
                }
                prop_assert_eq!(end, before.row_len(row) as usize);
            }

            let (at, len, insert) = edit;
            let start = before.clip_offset(at.min(before.len()), fe_text::Bias::Left);
            let end = before.clip_offset((start + len).min(before.len()), fe_text::Bias::Left);
            let mut after = before.clone();
            after.replace(start..end, &insert);
            let first_row = before.offset_to_point(start).row as usize;
            let old_rows = before.offset_to_point(end).row as usize - first_row + 1;
            let new_rows = after.offset_to_point(start + insert.len()).row as usize - first_row + 1;
            map.splice(&after, first_row, old_rows, new_rows, &mut Monospace);
            let mut fresh = WrapMap::new(&after, 4);
            fresh.rebuild(&after, Some(width), 4, &mut Monospace);
            wrap_all(&mut fresh, &after);
            prop_assert_eq!(rows(&map, &after), rows(&fresh, &after));
        }

        /// Wrapping with the cached ASCII widths, skipping rows that fit,
        /// breaks where measuring every char does.
        #[test]
        fn ascii_fast_path_matches_measuring(
            text in "[a-e~ \t\né]{0,120}",
            columns in 1usize..40,
            scale in prop::sample::select(vec![0.1f32, 0.3, 1., 7.3]),
        ) {
            let text = Text::new(&text);
            let width = columns as f32 * scale;
            let mut widths = Uneven { scale };
            let ascii = AsciiWidths::new(width, &mut widths);
            let mut wrapper = Wrapper::new(width, 4, &ascii, &mut widths);
            for row in 0..text.row_count() {
                let (LineItem::Wrapped(breaks), _) = wrapper.wrap_row(&text, row) else {
                    unreachable!();
                };
                let start = text.row_start(row);
                let line = text.slice_to_string(start..start + text.row_len(row) as usize);
                let mut measured = Uneven { scale };
                let want = break_points(&line, width, 4, |ch| measured.char_width(ch));
                prop_assert_eq!(breaks, want, "row {:?}", line);
            }
        }
    }
}
