//! LSP positions and byte offsets (docs/decisions/lsp-client.md §4.2,
//! §4.3).
//!
//! fe-lsp offers `["utf-8", "utf-16"]` and nothing else, so there's no
//! utf-32 path. Both conversions go through [`Text`]'s rope metrics, so
//! none of them scans a row: a position at column 5,000,000 of a one-line
//! file costs what one at column 5 does.

use fe_text::{Point, PointUtf16, Text};
use lsp_types::Position;

/// The position encoding a server chose in `initialize` (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionEncoding {
    /// Columns count bytes: our own [`Point`].
    Utf8,
    /// Columns count UTF-16 code units. Also what a server that doesn't
    /// say means.
    Utf16,
}

/// The position of byte `offset` in `text`, which is clamped to the text.
/// `offset` should be on a char boundary, as every offset fe-lsp sends is.
pub fn to_lsp(text: &Text, offset: usize, encoding: PositionEncoding) -> Position {
    match encoding {
        PositionEncoding::Utf8 => {
            let Point { row, column } = text.offset_to_point(offset);
            Position::new(row, column)
        }
        PositionEncoding::Utf16 => {
            let PointUtf16 { row, column } = text.offset_to_point_utf16(offset);
            Position::new(row, column)
        }
    }
}

/// The byte offset of `position` in `text`. A position a server may send
/// but fe-lsp never produces is clamped: a column past its row's end to the
/// row's end, a row past the last to the text's end, and a column inside a
/// char (between its bytes, or its surrogates) to that char's start.
pub fn from_lsp(text: &Text, position: Position, encoding: PositionEncoding) -> usize {
    match encoding {
        PositionEncoding::Utf8 => {
            text.point_to_offset(Point::new(position.line, position.character))
        }
        PositionEncoding::Utf16 => {
            text.point_utf16_to_offset(PointUtf16::new(position.line, position.character))
        }
    }
}
