//! Maps buffer text to what's displayed: tabs expand within a row, long rows
//! soft wrap and folds hide rows ([`DisplayMap`]), and blocks of rows go
//! between display rows ([`BlockMap`]); one inlay per frame, and any number
//! of inlay hints, go inside a row ([`ExpandedRow::insert_inlay`],
//! [`ExpandedRow::insert_hints`]). [`layout`] lays
//! out a frame of an editor from that, [`minimap`] the minimap beside it, and
//! [`sticky`] the rows sticky scroll pins at its top. Must never depend on
//! GPUI.
//!
//! Columns are byte offsets within a row, in buffer or display coordinates.

mod block_map;
mod display_map;
mod fold_map;
pub mod layout;
mod long_row;
pub mod minimap;
pub mod sticky;
mod wrap_map;

pub use block_map::{Block, BlockMap, ScreenRow};
pub use display_map::{DisplayMap, DisplayRow, WRAP_SLICE_BYTES};
pub use fe_text::Bias;
pub use fold_map::Fold;
pub use long_row::{LongRows, MAX_CLUSTER_BYTES, RowIndex, RowSlice, SourceKey};
pub use sticky::{StickyRow, sticky_rows};

/// Measures chars for soft wrapping, in the unit of the wrap width (the UI
/// uses pixels).
pub trait CharWidth {
    fn char_width(&mut self, ch: char) -> f32;

    /// The width of a grapheme cluster of more than one char, such as an
    /// emoji sequence or a letter with combining marks, which a shaper may
    /// draw narrower or wider than its chars measured alone. Sums their
    /// widths unless overridden.
    fn cluster_width(&mut self, cluster: &str) -> f32 {
        cluster.chars().map(|ch| self.char_width(ch)).sum()
    }
}

/// Every char one unit wide, so the wrap width counts chars. For headless
/// use and tests.
#[derive(Copy, Clone, Debug, Default)]
pub struct Monospace;

impl CharWidth for Monospace {
    fn char_width(&mut self, _: char) -> f32 {
        1.
    }
}

/// One row with its tabs expanded to spaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandedRow {
    text: String,
    tabs: Vec<Tab>,
    /// The inlay and the hints, in display order.
    spans: Vec<InlaySpan>,
}

/// Where an inlay or an inlay hint sits in an [`ExpandedRow`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct InlaySpan {
    buffer_col: usize,
    /// The display column the span starts at.
    display_col: usize,
    len: usize,
    /// An inlay hint, rather than the inlay.
    hint: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Tab {
    buffer_col: usize,
    display_col: usize,
    width: usize,
}

impl ExpandedRow {
    /// Expands each tab to the next multiple of `tab_size` display columns,
    /// counting each char as one column.
    pub fn new(row: &str, tab_size: usize) -> Self {
        Self::with_start_column(row, 0, tab_size)
    }

    /// Expands part of a row whose first char is at display column
    /// `start_column`, so its tabs reach the same tab stops as in the whole
    /// row. Columns in the result count from the part's start.
    pub fn with_start_column(row: &str, start_column: usize, tab_size: usize) -> Self {
        let tab_size = tab_size.max(1);
        if !row.contains('\t') {
            return Self {
                text: row.to_string(),
                tabs: Vec::new(),
                spans: Vec::new(),
            };
        }
        let mut text = String::with_capacity(row.len() + 16);
        let mut tabs = Vec::new();
        let mut column = start_column;
        for (buffer_col, ch) in row.char_indices() {
            if ch == '\t' {
                let width = tab_size - column % tab_size;
                tabs.push(Tab {
                    buffer_col,
                    display_col: text.len(),
                    width,
                });
                text.extend(std::iter::repeat_n(' ', width));
                column += width;
            } else {
                text.push(ch);
                column += 1;
            }
        }
        Self {
            text,
            tabs,
            spans: Vec::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Inserts `text`, one line, at the display column of `buffer_col`,
    /// after any hint there. The row's later columns shift right by its
    /// length; `to_display` of `buffer_col` stays before it.
    pub fn insert_inlay(&mut self, buffer_col: usize, text: &str) {
        debug_assert!(self.inlay_range().is_none(), "a row takes one inlay");
        debug_assert!(!text.contains('\n'), "an inlay is one line");
        self.insert_span(buffer_col, text, false);
    }

    /// Inserts each hint's text, one line, at the display column of its
    /// buffer column, in column order (ties in the order given), before any
    /// inlay at the same column. Like the inlay, a hint shifts the row's
    /// later columns right by its length, and `to_display` of its column
    /// stays before it.
    pub fn insert_hints(&mut self, hints: &[(usize, &str)]) {
        let mut sorted: Vec<_> = hints.iter().collect();
        sorted.sort_by_key(|(buffer_col, _)| *buffer_col);
        for (buffer_col, text) in sorted {
            debug_assert!(!text.contains('\n'), "a hint is one line");
            self.insert_span(*buffer_col, text, true);
        }
    }

    /// Inserts a span after the hints at `buffer_col`, and after the inlay
    /// there too unless it's a hint.
    fn insert_span(&mut self, buffer_col: usize, text: &str, hint: bool) {
        let ix = self.spans.partition_point(|span| {
            span.buffer_col < buffer_col || span.buffer_col == buffer_col && (span.hint || !hint)
        });
        let before: usize = self.spans[..ix].iter().map(|span| span.len).sum();
        let display_col = self.expand(buffer_col) + before;
        self.text.insert_str(display_col, text);
        for span in &mut self.spans[ix..] {
            span.display_col += text.len();
        }
        self.spans.insert(
            ix,
            InlaySpan {
                buffer_col,
                display_col,
                len: text.len(),
                hint,
            },
        );
    }

    /// The display columns the inlay covers, if the row has one.
    pub fn inlay_range(&self) -> Option<std::ops::Range<usize>> {
        let inlay = self.spans.iter().find(|span| !span.hint)?;
        Some(inlay.display_col..inlay.display_col + inlay.len)
    }

    /// The display columns each hint covers, in display order.
    pub fn hint_ranges(&self) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
        (self.spans.iter())
            .filter(|span| span.hint)
            .map(|span| span.display_col..span.display_col + span.len)
    }

    /// The display column of `buffer_col` with tabs expanded, before any
    /// span is counted.
    fn expand(&self, buffer_col: usize) -> usize {
        let before = self.tabs.partition_point(|tab| tab.buffer_col < buffer_col);
        let extra: usize = self.tabs[..before].iter().map(|tab| tab.width - 1).sum();
        buffer_col + extra
    }

    pub fn to_display(&self, buffer_col: usize) -> usize {
        let spans: usize = (self.spans.iter())
            .take_while(|span| span.buffer_col < buffer_col)
            .map(|span| span.len)
            .sum();
        self.expand(buffer_col) + spans
    }

    /// The buffer column for a display column. Columns inside an expanded tab
    /// snap to before (`Left`) or after (`Right`) it. Columns inside an inlay
    /// or a hint, or at either edge of it, give the buffer column it was
    /// inserted at.
    pub fn to_buffer(&self, display_col: usize, bias: Bias) -> usize {
        let mut spans = 0;
        for span in &self.spans {
            if display_col < span.display_col {
                break;
            }
            if display_col <= span.display_col + span.len {
                return span.buffer_col;
            }
            spans += span.len;
        }
        let display_col = display_col - spans;
        let mut extra = 0;
        for tab in &self.tabs {
            if display_col <= tab.display_col {
                break;
            }
            if display_col < tab.display_col + tab.width {
                return match bias {
                    Bias::Left => tab.buffer_col,
                    Bias::Right => tab.buffer_col + 1,
                };
            }
            extra += tab.width - 1;
        }
        display_col - extra
    }

    /// The column, in chars, of a display byte offset. Up/down movement keeps
    /// this column.
    pub fn char_column(&self, display_col: usize) -> usize {
        self.text[..display_col.min(self.text.len())]
            .chars()
            .count()
    }

    /// The display byte offset of a char column, clamped to the row's end.
    pub fn display_col_for_char_column(&self, column: usize) -> usize {
        self.text
            .char_indices()
            .nth(column)
            .map_or(self.text.len(), |(ix, _)| ix)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn expands_to_tab_stops() {
        let row = ExpandedRow::new("a\tbc\td", 4);
        assert_eq!(row.text(), "a   bc  d");
        assert_eq!(row.to_display(1), 1);
        assert_eq!(row.to_display(2), 4);
        assert_eq!(row.to_display(5), 8);
        assert_eq!(row.to_buffer(2, Bias::Left), 1);
        assert_eq!(row.to_buffer(2, Bias::Right), 2);
        assert_eq!(row.to_buffer(4, Bias::Left), 2);
        assert_eq!(row.to_buffer(8, Bias::Left), 5);
    }

    #[test]
    fn counts_chars_not_bytes() {
        let row = ExpandedRow::new("é\tx", 4);
        assert_eq!(row.text(), "é   x");
        assert_eq!(row.char_column(row.to_display(3)), 4);
        assert_eq!(row.display_col_for_char_column(4), 5);
        assert_eq!(row.display_col_for_char_column(99), 6);
    }

    #[test]
    fn hints_go_before_the_inlay_at_their_column() {
        let mut row = ExpandedRow::new("a\tb", 4);
        row.insert_inlay(1, "XY");
        row.insert_hints(&[(2, "c"), (1, "h"), (1, "i")]);
        assert_eq!(row.text(), "ahiXY   cb");
        assert_eq!(row.inlay_range(), Some(3..5));
        assert_eq!(row.hint_ranges().collect::<Vec<_>>(), [1..2, 2..3, 8..9]);
        assert_eq!(row.to_display(1), 1);
        assert_eq!(row.to_display(2), 8);
        for display in 1..=5 {
            assert_eq!(row.to_buffer(display, Bias::Left), 1);
        }
        assert_eq!(row.to_buffer(6, Bias::Left), 1);
        assert_eq!(row.to_buffer(6, Bias::Right), 2);
        assert_eq!(row.to_buffer(8, Bias::Left), 2);
        assert_eq!(row.to_buffer(10, Bias::Left), 3);
    }

    proptest! {
        #[test]
        fn round_trips_past_hints(
            row in "[a\t é]{0,40}",
            tab_size in 1usize..9,
            hints in proptest::collection::vec((0usize..41, "[hé]{0,3}"), 0..6),
        ) {
            let mut expanded = ExpandedRow::new(&row, tab_size);
            let boundaries: Vec<_> = row.char_indices().map(|(ix, _)| ix).chain([row.len()]).collect();
            let hints: Vec<_> = (hints.iter())
                .map(|(col, text)| (boundaries[col % boundaries.len()], text.as_str()))
                .collect();
            expanded.insert_hints(&hints);
            let ranges: Vec<_> = expanded.hint_ranges().collect();
            for &buffer_col in &boundaries {
                let display = expanded.to_display(buffer_col);
                prop_assert!(expanded.text().is_char_boundary(display));
                prop_assert_eq!(expanded.to_buffer(display, Bias::Left), buffer_col);
                prop_assert!(!ranges.iter().any(|range| range.start < display && display < range.end));
            }
            let mut sorted = hints.clone();
            sorted.sort_by_key(|(col, _)| *col);
            for (range, (col, _)) in ranges.iter().zip(&sorted) {
                for display in range.clone() {
                    prop_assert_eq!(expanded.to_buffer(display, Bias::Right), *col);
                }
            }
        }

        #[test]
        fn round_trips(row in "[a\t é]{0,40}", tab_size in 1usize..9) {
            let expanded = ExpandedRow::new(&row, tab_size);
            prop_assert!(!expanded.text().contains('\t'));
            for (buffer_col, _) in row.char_indices().chain([(row.len(), ' ')]) {
                let display = expanded.to_display(buffer_col);
                prop_assert!(expanded.text().is_char_boundary(display));
                prop_assert_eq!(expanded.to_buffer(display, Bias::Left), buffer_col);
                prop_assert_eq!(expanded.to_buffer(display, Bias::Right), buffer_col);
            }
        }
    }
}
