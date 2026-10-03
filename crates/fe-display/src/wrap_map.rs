// Adapted from gpui-kit (https://github.com/longbridge/gpui-kit) at commit
// 2c5162f8c5b0c7fcec066ed53125d304c632bfe2, files
// crates/base/src/input/editor/display_map/{text_wrapper,wrap_map}.rs.
// Copyright 2024 - 2026 Longbridge. Licensed under the Apache License,
// Version 2.0 (see THIRD_PARTY.md).
//
// Modified for fastedit: GPUI-free (wrapping measures through `CharWidth`),
// lines are read from `fe_text::Text`, items store only their wrap points,
// and an unwrapped map keeps no tree at all.

//! Soft wrapping: buffer rows ↔ wrap rows.
//!
//! Each buffer row is one item in a [`SumTree`] holding the byte offsets
//! where its continuation rows start, so mapping between buffer rows and
//! wrap rows is a seek, and an edit re-wraps only the rows it touched.

use std::ops::Range;

use fe_text::Text;
use sum_tree::{Bias, ContextLessSummary, Dimension, Dimensions, Item, SumTree};

use crate::CharWidth;

#[derive(Clone, Debug)]
struct LineItem {
    /// Byte offsets within the row where continuation rows start, ascending.
    breaks: Box<[u32]>,
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
        LineSummary {
            buffer_rows: 1,
            wrap_rows: self.breaks.len() + 1,
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
    /// `None` doesn't wrap: every buffer row is one wrap row and `lines` is
    /// empty.
    width: Option<f32>,
    tab_size: usize,
    line_count: usize,
    lines: SumTree<LineItem>,
}

impl WrapMap {
    pub(crate) fn new(text: &Text, tab_size: usize) -> Self {
        Self {
            width: None,
            tab_size: tab_size.max(1),
            line_count: text.row_count() as usize,
            lines: SumTree::new(()),
        }
    }

    pub(crate) fn width(&self) -> Option<f32> {
        self.width
    }

    /// Re-wraps every row of `text` at `width`.
    pub(crate) fn rebuild(
        &mut self,
        text: &Text,
        width: Option<f32>,
        tab_size: usize,
        widths: &mut dyn CharWidth,
    ) {
        self.width = width;
        self.tab_size = tab_size.max(1);
        self.line_count = text.row_count() as usize;
        self.lines = match width {
            None => SumTree::new(()),
            Some(width) => {
                let mut wrapper = Wrapper::new(width, self.tab_size, widths);
                let items = (0..text.row_count()).map(|row| wrapper.wrap_row(text, row));
                SumTree::from_iter(items, ())
            }
        };
    }

    /// Replaces `old_rows` rows starting at `start` with `new_rows` rows of
    /// `text`, which is the text after the edit.
    pub(crate) fn splice(
        &mut self,
        text: &Text,
        start: usize,
        old_rows: usize,
        new_rows: usize,
        widths: &mut dyn CharWidth,
    ) {
        self.line_count = self.line_count + new_rows - old_rows;
        let Some(width) = self.width else {
            return;
        };
        let mut wrapper = Wrapper::new(width, self.tab_size, widths);
        let new_lines = (start..start + new_rows).map(|row| wrapper.wrap_row(text, row as u32));
        let mut cursor = self.lines.cursor::<BufferRows>(());
        let mut lines = cursor.slice(&BufferRows(start), Bias::Right);
        cursor.seek_forward(&BufferRows(start + old_rows), Bias::Right);
        lines.extend(new_lines, ());
        lines.append(cursor.suffix(), ());
        drop(cursor);
        self.lines = lines;
        debug_assert_eq!(self.lines.summary().buffer_rows, self.line_count);
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
        cursor.start().1.0
    }

    /// The wrap row showing byte `column` of `buffer_row`. A wrap point
    /// starts the row after it.
    pub(crate) fn row_for(&self, buffer_row: usize, column: usize) -> usize {
        if self.width.is_none() {
            return buffer_row;
        }
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(());
        cursor.seek(&BufferRows(buffer_row), Bias::Right);
        let first = cursor.start().1.0;
        let wrapped = cursor.item().map_or(0, |line| {
            line.breaks.partition_point(|&at| at as usize <= column)
        });
        first + wrapped
    }

    /// Where wrap row `row` (clamped to the last) is.
    pub(crate) fn segment(&self, text: &Text, row: usize) -> WrapSegment {
        let row = row.min(self.row_count().saturating_sub(1));
        let (buffer_row, breaks, index) = if self.width.is_none() {
            (row, &[][..], 0)
        } else {
            let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(());
            cursor.seek(&WrapRows(row), Bias::Right);
            let line = cursor.item().expect("row is in range");
            (
                cursor.start().1.0,
                &line.breaks[..],
                row - cursor.start().0.0,
            )
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
}

/// Finds wrap points, caching the widths of ASCII chars.
struct Wrapper<'a> {
    width: f32,
    tab_size: usize,
    widths: &'a mut dyn CharWidth,
    ascii: [f32; 128],
    line: String,
}

impl<'a> Wrapper<'a> {
    fn new(width: f32, tab_size: usize, widths: &'a mut dyn CharWidth) -> Self {
        let mut ascii = [0.; 128];
        for (ch, slot) in (0..128u8).map(char::from).zip(&mut ascii) {
            *slot = widths.char_width(ch);
        }
        Self {
            width,
            tab_size,
            widths,
            ascii,
            line: String::new(),
        }
    }

    fn char_width(&mut self, ch: char) -> f32 {
        match self.ascii.get(ch as usize) {
            Some(&width) => width,
            None => self.widths.char_width(ch),
        }
    }

    fn wrap_row(&mut self, text: &Text, row: u32) -> LineItem {
        let start = text.row_start(row);
        let mut line = std::mem::take(&mut self.line);
        line.clear();
        line.extend(text.chunks_in_range(start..start + text.row_len(row) as usize));
        let breaks = self.wrap_points(&line);
        self.line = line;
        LineItem { breaks }
    }

    /// Breaks `line` into rows no wider than the wrap width, after the
    /// whitespace before a word where possible, otherwise mid-word. Every
    /// row keeps at least one char, and whitespace may hang past the edge.
    /// Tab stops count from the start of each row, as the rows are laid
    /// out.
    fn wrap_points(&mut self, line: &str) -> Box<[u32]> {
        let space = self.char_width(' ');
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
                let (width, columns) = if ch == '\t' {
                    let columns = self.tab_size - column % self.tab_size;
                    (space * columns as f32, columns)
                } else {
                    (self.char_width(ch), 1)
                };
                if ix > 0 && !is_space && x + width > self.width {
                    row_start += word_start.unwrap_or(ix);
                    breaks.push(row_start as u32);
                    continue 'rows;
                }
                x += width;
                column += columns;
                after_space = is_space;
            }
            break;
        }
        breaks.into()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::Monospace;

    fn wrapped(text: &str, width: f32) -> Vec<Vec<String>> {
        let text = Text::new(text);
        let mut map = WrapMap::new(&text, 4);
        map.rebuild(&text, Some(width), 4, &mut Monospace);
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
            prop_assert_eq!(rows(&map, &after), rows(&fresh, &after));
        }
    }
}
