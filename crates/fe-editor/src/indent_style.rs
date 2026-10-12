//! Which indentation a file already uses: tabs, or a width of spaces,
//! guessed from its first rows when it opens.

use fe_buffer::Text;

/// How a file indents.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    /// A width of spaces, from 2 to 8.
    Spaces(u8),
}

/// The most rows [`detect_indent_style`] reads.
const MAX_ROWS: u32 = 10_000;

/// The fewest indented rows [`detect_indent_style`] decides from.
const MIN_INDENTED: usize = 5;

/// The indentation the first [`MAX_ROWS`] rows of `text` use, or `None`
/// with too few indented rows to tell.
///
/// Rows starting with a tab count for tabs. For rows indented with spaces,
/// the width is the most common increase in leading spaces from the
/// non-blank row before, from 2 to 8, ties going to the smaller. Tabs win
/// when more rows start with a tab than with two or more spaces. Blank rows
/// count for nothing.
pub fn detect_indent_style(text: &Text) -> Option<IndentStyle> {
    let mut counts = Counts::default();
    let mut row = Row::default();
    let mut rows = 0;
    'chunks: for chunk in text.chunks() {
        let bytes = chunk.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if row.past_indent {
                // The rest of the row tells nothing: skip to its end.
                match bytes[i..].iter().position(|&byte| byte == b'\n') {
                    Some(at) => i += at,
                    None => continue 'chunks,
                }
            }
            let byte = bytes[i];
            i += 1;
            match byte {
                b'\n' => {
                    counts.add(&row);
                    row = Row::default();
                    rows += 1;
                    if rows == MAX_ROWS {
                        break 'chunks;
                    }
                }
                b' ' | b'\t' | b'\r' => {
                    if row.first.is_none() {
                        row.first = Some(byte);
                    }
                    if byte == b' ' && row.only_spaces {
                        row.spaces += 1;
                    } else {
                        row.only_spaces = false;
                    }
                }
                _ => {
                    row.first.get_or_insert(byte);
                    row.past_indent = true;
                }
            }
        }
    }
    if rows < MAX_ROWS {
        counts.add(&row);
    }
    counts.style()
}

/// The row being read.
struct Row {
    first: Option<u8>,
    /// Leading spaces.
    spaces: usize,
    /// Whether only spaces have come so far.
    only_spaces: bool,
    /// Whether a non-blank byte has come.
    past_indent: bool,
}

impl Default for Row {
    fn default() -> Self {
        Row {
            first: None,
            spaces: 0,
            only_spaces: true,
            past_indent: false,
        }
    }
}

#[derive(Default)]
struct Counts {
    tab_rows: usize,
    /// Rows starting with two or more spaces.
    space_rows: usize,
    /// Rows starting with a tab or a space.
    indented: usize,
    /// How often leading spaces grew by each width, 2 to 8.
    increases: [usize; 7],
    /// The leading spaces of the last non-blank row not starting with a tab.
    previous: usize,
}

impl Counts {
    fn add(&mut self, row: &Row) {
        if !row.past_indent {
            return;
        }
        match row.first {
            Some(b'\t') => {
                self.tab_rows += 1;
                self.indented += 1;
                return;
            }
            Some(b' ') => self.indented += 1,
            _ => {}
        }
        if row.spaces >= 2 {
            self.space_rows += 1;
        }
        if let Some(increase @ 2..=8) = row.spaces.checked_sub(self.previous) {
            self.increases[increase - 2] += 1;
        }
        self.previous = row.spaces;
    }

    fn style(&self) -> Option<IndentStyle> {
        if self.indented < MIN_INDENTED {
            return None;
        }
        if self.tab_rows > self.space_rows {
            return Some(IndentStyle::Tabs);
        }
        let (width, &count) = self
            .increases
            .iter()
            .enumerate()
            .rev()
            .max_by_key(|&(_, count)| count)?;
        (count > 0).then_some(IndentStyle::Spaces(width as u8 + 2))
    }
}

#[cfg(test)]
#[path = "indent_style_tests.rs"]
mod tests;
