//! A `.editorconfig` `charset` decides how a file whose bytes are
//! ambiguous opens, and saves; bytes that clearly say otherwise win.

use std::path::Path;

use fe_buffer::{Buffer, Encoding, FileEncoding};

use super::read;

/// `bytes` written as `name` in `dir`, under `charset` (no `.editorconfig`
/// if `None`), read as a pane reads it.
fn opened(dir: &Path, charset: Option<&str>, name: &str, bytes: &[u8]) -> Buffer {
    let config = dir.join(".editorconfig");
    match charset {
        Some(charset) => {
            std::fs::write(&config, format!("root = true\n[*]\ncharset = {charset}\n")).unwrap()
        }
        None => {
            std::fs::remove_file(&config).ok();
        }
    }
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    read(path).unwrap().expect("the file exists")
}

fn saved(buffer: &Buffer) -> Vec<u8> {
    let mut out = Vec::new();
    buffer.write_to(&mut out).unwrap();
    out
}

fn encoding(label: &str, bom: bool) -> FileEncoding {
    FileEncoding {
        encoding: Encoding::for_label(label.as_bytes()).unwrap(),
        bom,
    }
}

#[test]
fn charset_decides_ambiguous_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let utf8 = b"caf\xC3\xA9\n";

    let latin1 = opened(dir, Some("latin1"), "a.txt", utf8);
    assert_eq!(latin1.text().to_string(), "cafÃ©\n");
    assert_eq!(latin1.encoding(), encoding("windows-1252", false));
    assert_eq!(latin1.encoding().label(), "windows-1252");
    assert!(!latin1.is_dirty());
    assert_eq!(saved(&latin1), utf8);

    let as_utf8 = opened(dir, Some("utf-8"), "b.txt", utf8);
    assert_eq!(as_utf8.text().to_string(), "café\n");
    assert_eq!(as_utf8.encoding(), FileEncoding::UTF8);
    assert!(!as_utf8.is_dirty());

    let bom = opened(dir, Some("utf-8-bom"), "c.txt", b"plain\n");
    assert_eq!(bom.text().to_string(), "plain\n");
    assert_eq!(bom.encoding(), encoding("utf-8", true));
    assert!(bom.is_dirty(), "the BOM it lacks needs saving");
    assert_eq!(saved(&bom), b"\xEF\xBB\xBFplain\n");

    // No .editorconfig: as detected, as before.
    let detected = opened(dir, None, "d.txt", utf8);
    assert_eq!(detected.text().to_string(), "café\n");
    assert_eq!(detected.encoding(), FileEncoding::UTF8);
    assert!(!detected.is_dirty());
    let detected = opened(dir, None, "e.txt", b"caf\xE9\n");
    assert_eq!(detected.text().to_string(), "café\n");
    assert_eq!(detected.encoding(), encoding("windows-1252", false));
}

#[test]
fn clear_bytes_keep_detection() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();

    let bom = opened(dir, Some("latin1"), "a.txt", b"\xEF\xBB\xBFcaf\xC3\xA9\n");
    assert_eq!(bom.text().to_string(), "café\n");
    assert_eq!(bom.encoding(), encoding("utf-8", true));
    assert!(!bom.is_dirty());

    let invalid = b"\xE9t\xE9 caf\xE9\n";
    let detected = Buffer::from_reader_detecting(&invalid[..]).unwrap();
    let not_utf8 = opened(dir, Some("utf-8"), "b.txt", invalid);
    assert_eq!(not_utf8.encoding(), detected.encoding());
    assert_ne!(not_utf8.encoding(), FileEncoding::UTF8);
    assert_eq!(not_utf8.text().to_string(), detected.text().to_string());
    assert!(!not_utf8.text().to_string().contains('\u{fffd}'));
    assert!(!not_utf8.is_dirty());

    let utf16: Vec<u8> = "hello\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let zero_bytes = opened(dir, Some("utf-8"), "c.txt", &utf16);
    assert_eq!(zero_bytes.text().to_string(), "hello\n");
    assert_eq!(zero_bytes.encoding(), encoding("utf-16le", false));
    assert!(!zero_bytes.is_dirty());

    let mut le_bom = vec![0xFF, 0xFE];
    le_bom.extend(&utf16);
    let le = opened(dir, Some("utf-16be"), "d.txt", &le_bom);
    assert_eq!(le.text().to_string(), "hello\n");
    assert_eq!(le.encoding(), encoding("utf-16le", true));
    assert!(!le.is_dirty());
    assert_eq!(saved(&le), le_bom);
}
