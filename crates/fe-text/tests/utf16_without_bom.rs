//! UTF-16 files with no BOM are recognised by their zero bytes.

use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};
use fe_text::{DecodedFile, FileEncoding, LineEnding, Text};

fn encode(text: &str, big_endian: bool) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(|unit| {
            if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            }
        })
        .collect()
}

#[test]
fn utf16_without_a_bom_is_read_by_its_zero_bytes() {
    for (big_endian, expected) in [(false, UTF_16LE), (true, UTF_16BE)] {
        let bytes = encode("hello\r\nworld\r\n", big_endian);
        let DecodedFile {
            text,
            line_ending,
            encoding,
            replaced,
        } = Text::from_reader_detecting(&bytes[..]).unwrap();
        assert_eq!(
            encoding,
            FileEncoding {
                encoding: expected,
                bom: false
            }
        );
        assert_eq!(text.to_string(), "hello\nworld\n");
        assert_eq!(line_ending, LineEnding::CrLf);
        assert!(!replaced);
        assert_eq!(FileEncoding::detect(&bytes[..]).unwrap(), encoding);

        let mut out = Vec::new();
        text.write_encoded(line_ending, encoding, &mut out).unwrap();
        assert_eq!(out, bytes);
    }
}

#[test]
fn zero_bytes_out_of_the_utf16_pattern_stay_utf8() {
    let decoded = Text::from_reader_detecting(&b"a\0\0b"[..]).unwrap();
    assert_eq!(decoded.encoding, FileEncoding::UTF8);
    assert_eq!(decoded.encoding.encoding, UTF_8);
    assert_eq!(decoded.text.to_string(), "a\0\0b");
    assert_eq!(
        FileEncoding::detect(&b"plain ascii"[..]).unwrap(),
        FileEncoding::UTF8
    );
}
