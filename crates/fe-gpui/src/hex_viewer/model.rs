//! A hex view's text, without GPUI: how a row of bytes reads, where each
//! byte's cells are in it, and the parsing of what the header's inputs take.
//!
//! A row is `offset | 16 hex bytes | ASCII`:
//!
//! ```text
//! 00000010  48 65 6C 6C 6F 00 0A 0D  FF 20 7E 7F 41 42 43 44  Hello.... ~.ABCD
//! ```
//!
//! The offset is [`OFFSET_DIGITS`] hex digits, or [`WIDE_OFFSET_DIGITS`] for
//! a file longer than `0xFFFF_FFFF` bytes. Two spaces part the columns, one
//! space each hex pair and two the 8th pair from the 9th. Bytes
//! `0x20..=0x7E` show as themselves in the ASCII column, any other as `.`.
//! A partial last row pads its hex column, so its ASCII column lines up.

use std::{fmt::Write as _, ops::Range};

/// Bytes in a row.
pub(crate) const ROW_BYTES: u64 = 16;

/// An offset's digits in a file of up to `0xFFFF_FFFF` bytes.
pub(crate) const OFFSET_DIGITS: usize = 8;

/// An offset's digits in a longer file.
pub(crate) const WIDE_OFFSET_DIGITS: usize = 16;

/// The spaces between two columns.
const COLUMN_GAP: usize = 2;

/// The hex column's chars: 16 pairs, a space after each but the last, and
/// one more after the 8th.
const HEX_CHARS: usize = 16 * 3;

/// Which of a row's columns a byte's cell is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Hex,
    Ascii,
}

/// Where a row's columns are, in chars from its start, for a file of a
/// given length. Every char is one mono cell wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Columns {
    /// The offset's digits.
    pub(crate) offset_digits: usize,
}

impl Columns {
    /// The columns of a file `len` bytes long.
    pub(crate) fn for_len(len: u64) -> Self {
        let offset_digits = if len > 0xFFFF_FFFF {
            WIDE_OFFSET_DIGITS
        } else {
            OFFSET_DIGITS
        };
        Self { offset_digits }
    }

    /// The char the hex column starts at.
    pub(crate) fn hex_start(&self) -> usize {
        self.offset_digits + COLUMN_GAP
    }

    /// The char the ASCII column starts at.
    pub(crate) fn ascii_start(&self) -> usize {
        self.hex_start() + HEX_CHARS + COLUMN_GAP
    }

    /// A whole row's chars.
    pub(crate) fn row_chars(&self) -> usize {
        self.ascii_start() + ROW_BYTES as usize
    }

    /// The chars of byte `ix` (0 to 15) of a row in `column`.
    pub(crate) fn cell(&self, column: Column, ix: usize) -> Range<usize> {
        match column {
            Column::Hex => {
                let start = self.hex_start() + 3 * ix + usize::from(ix >= 8);
                start..start + 2
            }
            Column::Ascii => {
                let start = self.ascii_start() + ix;
                start..start + 1
            }
        }
    }

    /// The column and byte (0 to 15) of a row nearest char `x`, which may
    /// be fractional or past either end. The offset column counts as the
    /// hex column's first byte, and the gap before the ASCII column splits
    /// between the two.
    pub(crate) fn hit(&self, x: f32) -> (Column, usize) {
        let ascii = self.ascii_start() as f32;
        let hex_end = (self.hex_start() + HEX_CHARS) as f32;
        if x >= ascii - (ascii - hex_end) / 2. {
            let ix = (x - ascii).floor().clamp(0., 15.);
            return (Column::Ascii, ix as usize);
        }
        let c = (x - self.hex_start() as f32).floor().max(0.) as usize;
        // Byte 8 starts a char later, after the wider gap.
        let ix = if c >= 3 * 8 { (c - 1) / 3 } else { c / 3 };
        (Column::Hex, ix.min(15))
    }
}

/// How byte `byte` shows in the ASCII column.
pub(crate) fn ascii(byte: u8) -> char {
    if (0x20..=0x7E).contains(&byte) {
        char::from(byte)
    } else {
        '.'
    }
}

/// The text of the row starting at `offset`, of `bytes` (at most 16), in
/// `columns`.
pub(crate) fn row_text(columns: Columns, offset: u64, bytes: &[u8]) -> String {
    debug_assert!(bytes.len() as u64 <= ROW_BYTES);
    let mut text = String::with_capacity(columns.row_chars());
    write!(text, "{offset:0width$X}", width = columns.offset_digits).ok();
    text.push_str("  ");
    for ix in 0..ROW_BYTES as usize {
        match bytes.get(ix) {
            Some(byte) => write!(text, "{byte:02X}").ok(),
            None => write!(text, "  ").ok(),
        };
        if ix == 7 {
            text.push(' ');
        }
        if ix < 15 {
            text.push(' ');
        }
    }
    text.push_str("  ");
    text.extend(bytes.iter().map(|&byte| ascii(byte)));
    text
}

/// The rows of a file `len` bytes long: a row per 16 bytes, the last maybe
/// partial, and none for an empty file.
pub(crate) fn row_count(len: u64) -> u64 {
    len.div_ceil(ROW_BYTES)
}

/// `0x1A2F`: an offset as the status bar and the inputs show it.
pub(crate) fn hex_offset(offset: u64) -> String {
    format!("0x{offset:X}")
}

/// `bytes` as uppercase hex pairs, a space between each: `DE AD BE EF`.
pub(crate) fn hex_pairs(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (ix, byte) in bytes.iter().enumerate() {
        if ix > 0 {
            text.push(' ');
        }
        write!(text, "{byte:02X}").ok();
    }
    text
}

/// Why an input's text isn't an offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OffsetError {
    /// Not a decimal number or `0x` hex.
    Unparsed,
    /// Past the last byte of a file this many bytes long.
    PastEnd { len: u64 },
}

/// The byte offset `text` names, decimal or `0x` hex, around spaces,
/// within a file `len` bytes long.
pub(crate) fn parse_offset(text: &str, len: u64) -> Result<u64, OffsetError> {
    let text = text.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse::<u64>(),
    };
    let offset = parsed.map_err(|_| OffsetError::Unparsed)?;
    if offset >= len {
        return Err(OffsetError::PastEnd { len });
    }
    Ok(offset)
}

/// Why a hex query isn't bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HexQueryError {
    /// An odd count of hex digits: the last byte is half there.
    OddDigits,
    /// This char isn't a hex digit or a space.
    NotHex(char),
}

/// The bytes a hex query names: pairs of hex digits in either case, spaces
/// anywhere between them (`de ad BE EF`, `deadbeef`).
pub(crate) fn parse_hex_query(text: &str) -> Result<Vec<u8>, HexQueryError> {
    let mut digits = Vec::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        let digit = ch.to_digit(16).ok_or(HexQueryError::NotHex(ch))?;
        digits.push(digit as u8);
    }
    if digits.len() % 2 == 1 {
        return Err(HexQueryError::OddDigits);
    }
    Ok(digits
        .chunks(2)
        .map(|pair| pair[0] << 4 | pair[1])
        .collect())
}
