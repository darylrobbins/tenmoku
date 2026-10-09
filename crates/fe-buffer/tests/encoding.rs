//! Buffers save back in the encoding they were read in, or refuse to.

use std::io::ErrorKind;

use fe_buffer::{Buffer, LineEnding, Origin, Text};
use fe_text::{Encoding, FileEncoding};

fn encoding(label: &str) -> &'static Encoding {
    Encoding::for_label(label.as_bytes()).unwrap()
}

fn legacy(label: &str, text: &str) -> Vec<u8> {
    let (bytes, _, unmappable) = encoding(label).encode(text);
    assert!(!unmappable, "fixture must be representable");
    bytes.into_owned()
}

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut bytes = if big_endian {
        vec![0xFE, 0xFF]
    } else {
        vec![0xFF, 0xFE]
    };
    for unit in text.encode_utf16() {
        let pair = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        bytes.extend_from_slice(&pair);
    }
    bytes
}

fn write(buffer: &Buffer) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    buffer.write_to(&mut out).map(|()| out)
}

const SAMPLE: &str = "héllo wörld → 日本語 😀\r\nsecond line\r\n";
const FRENCH: &str = "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\n\
                      Café naïve: élève, garçon, où, être, fenêtre, hôtel, Noël.\n";
const JAPANESE: &str = "日本語のテキストです。これはシフトJISで書かれたファイルです。\n\
                        東京は日本の首都です。今日はいい天気ですね。\n";

#[test]
fn detected_files_round_trip_byte_for_byte() {
    let mut utf8_bom = b"\xEF\xBB\xBF".to_vec();
    utf8_bom.extend_from_slice(SAMPLE.as_bytes());
    let files = [
        (SAMPLE.as_bytes().to_vec(), "UTF-8"),
        (utf8_bom, "UTF-8 with BOM"),
        (utf16(SAMPLE, false), "UTF-16LE"),
        (utf16(SAMPLE, true), "UTF-16BE"),
        (legacy("windows-1252", FRENCH), "windows-1252"),
        (legacy("shift_jis", JAPANESE), "Shift_JIS"),
    ];
    for (bytes, label) in files {
        let buffer = Buffer::from_reader_detecting(&bytes[..]).unwrap();
        assert_eq!(buffer.encoding().label(), label);
        assert!(write(&buffer).unwrap() == bytes, "{label}");
    }
}

#[test]
fn other_constructors_write_utf8_as_before() {
    let mut invalid = SAMPLE.as_bytes().to_vec();
    invalid.extend_from_slice(b"caf\xE9\r\n");
    let (text, line_ending) = Text::from_reader(&invalid[..]).unwrap();
    let buffers = [
        (
            Buffer::new("plain\ntext\n"),
            Text::new("plain\ntext\n"),
            LineEnding::Lf,
        ),
        (
            Buffer::from_file_contents(SAMPLE),
            Text::from_file_contents(SAMPLE).0,
            LineEnding::CrLf,
        ),
        // Bytes from_reader replaced are saved as U+FFFD, as they always were.
        (
            Buffer::from_reader(&invalid[..]).unwrap(),
            text,
            line_ending,
        ),
    ];
    for (buffer, text, line_ending) in buffers {
        assert_eq!(buffer.encoding(), FileEncoding::UTF8);
        assert_eq!(buffer.encoding().label(), "UTF-8");
        let mut before = Vec::new();
        text.write_to(line_ending, &mut before).unwrap();
        assert_eq!(write(&buffer).unwrap(), before);
    }
}

#[test]
fn unmappable_chars_are_refused_with_their_position() {
    let mut buffer = Buffer::from_reader_detecting(&legacy("windows-1252", FRENCH)[..]).unwrap();
    assert_eq!(buffer.encoding().label(), "windows-1252");
    // After "C" on line 2: 'ô' is one char but two bytes before the emoji.
    let at = buffer.text().row_start(1) + 1;
    buffer.edit([(at..at, "ô😀")], Origin::User);
    let error = write(&buffer).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
    let message = error.to_string();
    for part in ["U+1F600", "line 2, column 3", "windows-1252"] {
        assert!(message.contains(part), "{message:?} lacks {part:?}");
    }
    assert_eq!(
        message,
        "'😀' (U+1F600) on line 2, column 3 can't be saved in windows-1252"
    );
}

#[test]
fn replaced_bytes_block_saving_until_deleted() {
    // Valid UTF-8 for the sniffed 64 KiB, then a bad byte.
    let mut bytes = "b".repeat(64 * 1024 + 10).into_bytes();
    bytes.extend_from_slice(b" \xFF end\n");
    let mut buffer = Buffer::from_reader_detecting(&bytes[..]).unwrap();
    assert!(buffer.text().to_string().contains('\u{FFFD}'));

    let mut out = Vec::new();
    let error = buffer.write_to(&mut out).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
    assert!(error.to_string().contains("UTF-8"), "{error}");
    assert!(out.is_empty());

    let text = buffer.text().to_string();
    let at = text.find('\u{FFFD}').unwrap();
    buffer.edit([(at..at + '\u{FFFD}'.len_utf8(), "")], Origin::User);
    assert!(!buffer.text().to_string().contains('\u{FFFD}'));
    let mut expected = "b".repeat(64 * 1024 + 10).into_bytes();
    expected.extend_from_slice(b"  end\n");
    assert!(write(&buffer).unwrap() == expected);
}
