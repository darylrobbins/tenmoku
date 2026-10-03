//! Maps buffer text to what's displayed: tabs expand within a row, long rows
//! soft wrap and folds hide rows ([`DisplayMap`]); inlays and blocks come
//! later. [`layout`] lays out a frame of an editor from that. Must never
//! depend on GPUI.
//!
//! Columns are byte offsets within a row, in buffer or display coordinates.

mod display_map;
mod fold_map;
pub mod layout;
mod long_row;
mod wrap_map;

pub use display_map::{DisplayMap, DisplayRow};
pub use fe_text::Bias;
pub use fold_map::Fold;
pub use long_row::{LongRows, RowIndex, RowSlice};

/// Measures chars for soft wrapping, in the unit of the wrap width (the UI
/// uses pixels).
pub trait CharWidth {
    fn char_width(&mut self, ch: char) -> f32;
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
        Self { text, tabs }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn to_display(&self, buffer_col: usize) -> usize {
        let before = self.tabs.partition_point(|tab| tab.buffer_col < buffer_col);
        let extra: usize = self.tabs[..before].iter().map(|tab| tab.width - 1).sum();
        buffer_col + extra
    }

    /// The buffer column for a display column. Columns inside an expanded tab
    /// snap to before (`Left`) or after (`Right`) it.
    pub fn to_buffer(&self, display_col: usize, bias: Bias) -> usize {
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

    proptest! {
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
