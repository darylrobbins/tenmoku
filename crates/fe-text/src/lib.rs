//! Text storage for fastedit.
//!
//! This crate is the only place that names the underlying rope type. Everything
//! else talks to [`Text`] so the rope can be swapped (ropey 2 is still beta).
//! It must never depend on GPUI.
//!
//! Offsets are byte offsets into UTF-8 text. Text is stored with `\n` line
//! endings only; [`LineEnding`] records what the file used so it can be saved
//! back the same way.

use std::ops::Range;

use ropey::LineType;

const LF: LineType = LineType::LF;

/// Which side of an offset a position sticks to when text is inserted exactly there.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bias {
    /// Stay before text inserted at this offset.
    #[default]
    Left,
    /// Move after text inserted at this offset.
    Right,
}

/// A row and a byte column within that row. Both zero-based.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    pub row: u32,
    pub column: u32,
}

impl Point {
    pub const fn new(row: u32, column: u32) -> Self {
        Self { row, column }
    }
}

/// A row and a UTF-16 code-unit column, as used by LSP's default position encoding.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointUtf16 {
    pub row: u32,
    pub column: u32,
}

impl PointUtf16 {
    pub const fn new(row: u32, column: u32) -> Self {
        Self { row, column }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }

    /// Guess from the first line break; files mixing styles are normalised anyway.
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(ix) if ix > 0 && text.as_bytes()[ix - 1] == b'\r' => LineEnding::CrLf,
            _ => LineEnding::Lf,
        }
    }
}

/// Rope-backed text. Cloning is O(1) and shares storage, so a clone doubles as
/// an immutable snapshot that can be handed to a background thread.
#[derive(Clone, Default)]
pub struct Text {
    rope: ropey::Rope,
}

impl Text {
    /// Builds text from a string that already uses `\n` line endings.
    /// Use [`Text::from_file_contents`] for text read from disk.
    pub fn new(text: &str) -> Self {
        debug_assert!(
            !text.contains('\r'),
            "use Text::from_file_contents for CR line endings"
        );
        Self {
            rope: ropey::Rope::from_str(text),
        }
    }

    /// Normalises `\r\n` and lone `\r` to `\n` and reports the file's line ending.
    pub fn from_file_contents(text: &str) -> (Self, LineEnding) {
        let line_ending = LineEnding::detect(text);
        if !text.contains('\r') {
            return (Self::new(text), line_ending);
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        (Self::new(&normalized), line_ending)
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len() == 0
    }

    pub fn len_utf16(&self) -> usize {
        self.rope.len_utf16()
    }

    /// Number of rows. Always at least 1; a trailing newline starts an empty last row.
    pub fn row_count(&self) -> u32 {
        self.rope.len_lines(LF) as u32
    }

    pub fn max_point(&self) -> Point {
        self.offset_to_point(self.len())
    }

    /// Replaces `range` with `new_text`. The range must lie on char boundaries.
    pub fn replace(&mut self, range: Range<usize>, new_text: &str) {
        debug_assert!(!new_text.contains('\r'), "text must be normalised to LF");
        if !range.is_empty() {
            self.rope.remove(range.clone());
        }
        if !new_text.is_empty() {
            self.rope.insert(range.start, new_text);
        }
    }

    pub fn is_char_boundary(&self, offset: usize) -> bool {
        self.rope.is_char_boundary(offset)
    }

    /// Clamps `offset` to the text and moves it onto a char boundary in the
    /// direction of `bias`.
    pub fn clip_offset(&self, offset: usize, bias: Bias) -> usize {
        let offset = offset.min(self.len());
        match bias {
            Bias::Left => self.rope.floor_char_boundary(offset),
            Bias::Right => self.rope.ceil_char_boundary(offset),
        }
    }

    /// Byte offset of the start of `row`, clamped to the last row.
    pub fn row_start(&self, row: u32) -> usize {
        let row = (row as usize).min(self.rope.len_lines(LF) - 1);
        self.rope.line_to_byte_idx(row, LF)
    }

    /// Length of `row` in bytes, excluding its line break.
    pub fn row_len(&self, row: u32) -> u32 {
        let start = self.row_start(row);
        let end = if row + 1 < self.row_count() {
            self.row_start(row + 1) - 1
        } else {
            self.len()
        };
        (end - start) as u32
    }

    pub fn offset_to_point(&self, offset: usize) -> Point {
        let offset = offset.min(self.len());
        let row = self.rope.byte_to_line_idx(offset, LF);
        let row_start = self.rope.line_to_byte_idx(row, LF);
        Point::new(row as u32, (offset - row_start) as u32)
    }

    /// Converts a point to an offset, clamping the column to the row's length
    /// and onto a char boundary (towards the start).
    pub fn point_to_offset(&self, point: Point) -> usize {
        if point.row >= self.row_count() {
            return self.len();
        }
        let column = point.column.min(self.row_len(point.row)) as usize;
        self.clip_offset(self.row_start(point.row) + column, Bias::Left)
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.rope.byte_to_utf16_idx(offset.min(self.len()))
    }

    /// Converts a UTF-16 offset to a byte offset. Offsets inside a surrogate
    /// pair move to the start of that character.
    pub fn utf16_to_offset(&self, utf16: usize) -> usize {
        let utf16 = utf16.min(self.len_utf16());
        let offset = self.rope.utf16_to_byte_idx(utf16);
        self.clip_offset(offset, Bias::Left)
    }

    pub fn offset_to_point_utf16(&self, offset: usize) -> PointUtf16 {
        let offset = offset.min(self.len());
        let row = self.rope.byte_to_line_idx(offset, LF);
        let row_start = self.rope.line_to_byte_idx(row, LF);
        let column = self.rope.byte_to_utf16_idx(offset) - self.rope.byte_to_utf16_idx(row_start);
        PointUtf16::new(row as u32, column as u32)
    }

    /// Converts an LSP-style UTF-16 point to a byte offset, clamping the column
    /// to the row and out of surrogate pairs.
    pub fn point_utf16_to_offset(&self, point: PointUtf16) -> usize {
        if point.row >= self.row_count() {
            return self.len();
        }
        let row_start = self.row_start(point.row);
        let row_end = row_start + self.row_len(point.row) as usize;
        let row_start_utf16 = self.rope.byte_to_utf16_idx(row_start);
        let row_end_utf16 = self.rope.byte_to_utf16_idx(row_end);
        let target = (row_start_utf16 + point.column as usize).min(row_end_utf16);
        self.utf16_to_offset(target)
    }

    pub fn char_at(&self, offset: usize) -> Option<char> {
        (offset < self.len()).then(|| self.rope.char(offset))
    }

    /// Iterates the text's chunks, borrowed straight from the rope.
    pub fn chunks(&self) -> impl Iterator<Item = &str> {
        self.rope.chunks()
    }

    /// Iterates the chunks covering `range`, trimmed to it.
    pub fn chunks_in_range(&self, range: Range<usize>) -> impl Iterator<Item = &str> {
        self.rope.slice(range).chunks()
    }

    pub fn slice_to_string(&self, range: Range<usize>) -> String {
        let mut text = String::with_capacity(range.len());
        for chunk in self.chunks_in_range(range) {
            text.push_str(chunk);
        }
        text
    }

    /// Whether the text in `range` equals `expected`, without allocating.
    pub fn range_eq(&self, range: Range<usize>, expected: &str) -> bool {
        if range.len() != expected.len() || range.end > self.len() {
            return false;
        }
        let mut rest = expected.as_bytes();
        for chunk in self.chunks_in_range(range) {
            let (head, tail) = rest.split_at(chunk.len());
            if head != chunk.as_bytes() {
                return false;
            }
            rest = tail;
        }
        true
    }

    /// Text of `row` without its line break.
    pub fn row_to_string(&self, row: u32) -> String {
        let start = self.row_start(row);
        self.slice_to_string(start..start + self.row_len(row) as usize)
    }

    /// Serialises for saving, converting line endings back to `line_ending`.
    pub fn write_to(
        &self,
        line_ending: LineEnding,
        mut out: impl std::io::Write,
    ) -> std::io::Result<()> {
        for chunk in self.chunks() {
            match line_ending {
                LineEnding::Lf => out.write_all(chunk.as_bytes())?,
                LineEnding::CrLf => {
                    let mut lines = chunk.split('\n');
                    if let Some(first) = lines.next() {
                        out.write_all(first.as_bytes())?;
                    }
                    for line in lines {
                        out.write_all(b"\r\n")?;
                        out.write_all(line.as_bytes())?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chunk in self.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.to_string(), f)
    }
}

#[cfg(test)]
mod tests;
