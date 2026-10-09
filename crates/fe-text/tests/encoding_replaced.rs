//! `DecodedFile::replaced`: whether reading put U+FFFD where malformed input was.

use encoding_rs::{UTF_8, UTF_16LE, WINDOWS_1252};
use fe_text::{DecodedFile, Text};

fn detect(bytes: &[u8]) -> DecodedFile {
    Text::from_reader_detecting(bytes).unwrap()
}

fn utf16le(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

const SAMPLE: &str = "héllo wörld → 日本語 😀\nsecond line\n";

#[test]
fn clean_files_are_not_replaced() {
    let plain = detect(SAMPLE.as_bytes());
    assert_eq!(plain.encoding.encoding, UTF_8);
    assert!(!plain.encoding.bom);
    assert!(!plain.replaced);

    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(SAMPLE.as_bytes());
    let bom = detect(&bom);
    assert!(bom.encoding.bom);
    assert!(!bom.replaced);

    // A U+FFFD the file really holds isn't a replacement.
    let literal = detect("a \u{FFFD} b\n".as_bytes());
    assert_eq!(literal.text.to_string(), "a \u{FFFD} b\n");
    assert!(!literal.replaced);

    let utf16 = detect(&utf16le(SAMPLE.encode_utf16()));
    assert_eq!(utf16.encoding.encoding, UTF_16LE);
    assert_eq!(utf16.text.to_string(), SAMPLE);
    assert!(!utf16.replaced);

    let french = "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\n\
                  Café naïve: élève, garçon, où, être, fenêtre, hôtel, Noël.\n";
    let (bytes, _, unmappable) = WINDOWS_1252.encode(french);
    assert!(!unmappable);
    let legacy = detect(&bytes);
    assert_eq!(legacy.encoding.encoding, WINDOWS_1252);
    assert_eq!(legacy.text.to_string(), french);
    assert!(!legacy.replaced);
}

#[test]
fn malformed_input_is_replaced() {
    // Valid for the sniffed 64 KiB, so it's read as UTF-8 with one bad byte.
    let mut late_invalid = "b".repeat(64 * 1024 + 10).into_bytes();
    late_invalid.extend_from_slice(b" \xFF end\n");
    let utf8 = detect(&late_invalid);
    assert_eq!(utf8.encoding.encoding, UTF_8);
    assert!(utf8.text.to_string().ends_with(" \u{FFFD} end\n"));
    assert!(utf8.replaced);

    let mut units: Vec<u16> = "ab".encode_utf16().collect();
    units.insert(1, 0xD800);
    let utf16 = detect(&utf16le(units));
    assert_eq!(utf16.encoding.encoding, UTF_16LE);
    assert_eq!(utf16.text.to_string(), "a\u{FFFD}b");
    assert!(utf16.replaced);
}
