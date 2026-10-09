//! The read subset of [`Text`], for callers that must not care whether the
//! text is a rope or a mapped file.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use crate::{Bias, Point, Text};

/// Read-only text with `Text`'s meaning and units: byte offsets, `u32` rows
/// and [`Point`] columns in bytes.
///
/// Object-safe, so callers hold `&dyn TextSource` or `Arc<dyn TextSource>`.
///
/// Contract for every implementation:
///
/// - A line break is one byte (`\n`) or two (`\r\n`). [`Text`] always uses
///   one. Callers must use [`row_start`](Self::row_start) and
///   [`row_len`](Self::row_len), and never assume
///   `row_start(r + 1) == row_start(r) + row_len(r) + 1`.
/// - Offsets inside a two-byte line break are not char boundaries.
/// - Within one row, `slice_to_string(range).len() == range.len()`.
/// - Methods are infallible. A source that can fail, such as a file, answers
///   from what it could read and reports the failure through its own API.
pub trait TextSource: Send + Sync {
    /// Length in bytes.
    fn len(&self) -> usize;

    /// Number of rows; an empty text has one.
    fn row_count(&self) -> u32;

    /// Offset of the first byte of `row`.
    fn row_start(&self, row: u32) -> usize;

    /// Length of `row` in bytes, excluding its line break.
    fn row_len(&self, row: u32) -> u32;

    fn clip_offset(&self, offset: usize, bias: Bias) -> usize;

    fn is_char_boundary(&self, offset: usize) -> bool;

    fn char_at(&self, offset: usize) -> Option<char>;

    /// The chunks covering `range`, trimmed to it. A rope yields borrowed
    /// chunks; a file-backed source yields owned ones.
    fn chunks_in_range(&self, range: Range<usize>) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_>;

    /// A cheap owned handle to the same text.
    fn share(&self) -> Arc<dyn TextSource>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn offset_to_point(&self, offset: usize) -> Point {
        let offset = offset.min(self.len());
        let (mut low, mut high) = (0, self.row_count() - 1);
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            if self.row_start(mid) <= offset {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        Point::new(low, (offset - self.row_start(low)) as u32)
    }

    fn point_to_offset(&self, point: Point) -> usize {
        if point.row >= self.row_count() {
            return self.len();
        }
        let column = point.column.min(self.row_len(point.row)) as usize;
        self.clip_offset(self.row_start(point.row) + column, Bias::Left)
    }

    fn max_point(&self) -> Point {
        let row = self.row_count() - 1;
        Point::new(row, self.row_len(row))
    }

    fn slice_to_string(&self, range: Range<usize>) -> String {
        let mut text = String::with_capacity(range.len());
        for chunk in self.chunks_in_range(range) {
            text.push_str(&chunk);
        }
        text
    }

    /// Text of `row` without its line break.
    fn row_to_string(&self, row: u32) -> String {
        let start = self.row_start(row);
        self.slice_to_string(start..start + self.row_len(row) as usize)
    }
}

impl TextSource for Text {
    fn len(&self) -> usize {
        Text::len(self)
    }

    fn row_count(&self) -> u32 {
        Text::row_count(self)
    }

    fn row_start(&self, row: u32) -> usize {
        Text::row_start(self, row)
    }

    fn row_len(&self, row: u32) -> u32 {
        Text::row_len(self, row)
    }

    fn offset_to_point(&self, offset: usize) -> Point {
        Text::offset_to_point(self, offset)
    }

    fn point_to_offset(&self, point: Point) -> usize {
        Text::point_to_offset(self, point)
    }

    fn max_point(&self) -> Point {
        Text::max_point(self)
    }

    fn clip_offset(&self, offset: usize, bias: Bias) -> usize {
        Text::clip_offset(self, offset, bias)
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        Text::is_char_boundary(self, offset)
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        Text::char_at(self, offset)
    }

    fn chunks_in_range(&self, range: Range<usize>) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
        Box::new(Text::chunks_in_range(self, range).map(Cow::Borrowed))
    }

    fn slice_to_string(&self, range: Range<usize>) -> String {
        Text::slice_to_string(self, range)
    }

    fn row_to_string(&self, row: u32) -> String {
        Text::row_to_string(self, row)
    }

    fn share(&self) -> Arc<dyn TextSource> {
        Arc::new(self.clone())
    }
}
