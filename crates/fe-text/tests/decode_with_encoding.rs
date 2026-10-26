//! `Text::from_reader_with`: decoding in an encoding given rather than
//! detected.

use std::io::Read;

use encoding_rs::{SHIFT_JIS, UTF_8, UTF_16LE, WINDOWS_1252};
use fe_text::{FileEncoding, Text};

fn with(bytes: &[u8], encoding: &'static encoding_rs::Encoding) -> fe_text::DecodedFile {
    let encoding = FileEncoding {
        encoding,
        bom: false,
    };
    Text::from_reader_with(bytes, encoding).unwrap()
}

#[test]
fn decodes_in_the_encoding_given() {
    let latin = with(b"caf\xe9\n", WINDOWS_1252);
    assert_eq!(latin.text.to_string(), "café\n");
    assert_eq!(latin.encoding.encoding, WINDOWS_1252);
    assert!(!latin.replaced);

    let utf8 = with(b"caf\xe9\n", UTF_8);
    assert_eq!(utf8.text.to_string(), "caf\u{FFFD}\n");
    assert_eq!(utf8.encoding, FileEncoding::UTF8);
    assert!(utf8.replaced);

    // Not what detection would pick: chardetng doesn't say Shift_JIS here.
    let sjis = with(b"caf\xe9 au lait\n", SHIFT_JIS);
    assert_eq!(sjis.encoding.encoding, SHIFT_JIS);
    assert_ne!(sjis.text.to_string(), "café au lait\n");
}

#[test]
fn its_own_bom_is_dropped_and_recorded() {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "héllo\r\nwörld".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let decoded = with(&bytes, UTF_16LE);
    assert_eq!(decoded.text.to_string(), "héllo\nwörld");
    assert_eq!(decoded.line_ending, fe_text::LineEnding::CrLf);
    assert_eq!(
        decoded.encoding,
        FileEncoding {
            encoding: UTF_16LE,
            bom: true
        }
    );
    assert!(!decoded.replaced);

    // Asked for with a BOM, the file without one: the file's word goes.
    let plain = Text::from_reader_with(
        &bytes[2..],
        FileEncoding {
            encoding: UTF_16LE,
            bom: true,
        },
    )
    .unwrap();
    assert_eq!(plain.text.to_string(), "héllo\nwörld");
    assert!(!plain.encoding.bom);

    let utf8 = with(b"\xEF\xBB\xBFcaf\xC3\xA9", UTF_8);
    assert_eq!(utf8.text.to_string(), "café");
    assert!(utf8.encoding.bom);
    assert_eq!(utf8.encoding.label(), "UTF-8 with BOM");
}

#[test]
fn another_encodings_bom_is_text() {
    // UTF-8's BOM read as windows-1252.
    let decoded = with(b"\xEF\xBB\xBFa", WINDOWS_1252);
    assert_eq!(decoded.text.to_string(), "ï»¿a");
    assert!(!decoded.encoding.bom);

    // UTF-16LE's read as UTF-8 is malformed.
    let decoded = with(b"\xFF\xFEa", UTF_8);
    assert_eq!(decoded.text.to_string(), "\u{FFFD}\u{FFFD}a");
    assert!(decoded.replaced);
}

#[test]
fn short_files_decode() {
    assert_eq!(with(b"", WINDOWS_1252).text.to_string(), "");
    assert_eq!(with(b"\xe9", WINDOWS_1252).text.to_string(), "é");
    assert_eq!(with(b"\xEF\xBB", UTF_8).text.to_string(), "\u{FFFD}");
}

/// Hands out at most `step` bytes per read, so a decode sees its input cut
/// at awkward places.
struct Trickle<'a> {
    bytes: &'a [u8],
    step: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(buf.len()).min(self.bytes.len());
        buf[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}

#[test]
fn a_stream_over_64_kib_decodes_as_one_piece_does() {
    // Two-byte Shift_JIS chars and CRLFs, so the 64 KiB read buffer's edge
    // falls inside one at some point, past 64 KiB.
    let line = "日本語のテキスト、windows でも\r\n";
    let text = line.repeat(8192);
    let (encoded, _, unmappable) = SHIFT_JIS.encode(&text);
    assert!(!unmappable);
    assert!(encoded.len() > 3 * 64 * 1024);
    let encoding = FileEncoding {
        encoding: SHIFT_JIS,
        bom: false,
    };

    let whole = text.replace("\r\n", "\n");
    for step in [1 << 20, 64 * 1024 + 1, 4093] {
        let reader = Trickle {
            bytes: &encoded,
            step,
        };
        let decoded = Text::from_reader_with(reader, encoding).unwrap();
        assert_eq!(decoded.text.to_string(), whole, "reads of {step} bytes");
        assert_eq!(decoded.line_ending, fe_text::LineEnding::CrLf);
        assert!(!decoded.replaced);
    }

    // UTF-8 too, split inside a char, with a malformed byte past 64 KiB.
    let mut utf8 = "é€😀\n".repeat(20_000).into_bytes();
    utf8.extend_from_slice(b"\xFFend");
    let one = Text::from_reader_with(&utf8[..], FileEncoding::UTF8).unwrap();
    let streamed = Text::from_reader_with(
        Trickle {
            bytes: &utf8,
            step: 4093,
        },
        FileEncoding::UTF8,
    )
    .unwrap();
    assert_eq!(streamed.text.to_string(), one.text.to_string());
    assert!(one.replaced && streamed.replaced);
    assert!(one.text.to_string().ends_with("\u{FFFD}end"));
}
