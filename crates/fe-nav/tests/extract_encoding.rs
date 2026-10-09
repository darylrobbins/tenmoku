//! `extract` in files the editor decodes: offsets are into the decoded text.

use std::{fs, path::Path};

use fe_buffer::Buffer;
use fe_nav::extract;

/// Extracts `bytes` as `a.rs` and checks that `target`'s name range selects
/// `target` in the detected text, on `row`. Returns the detected encoding.
fn check_target(bytes: &[u8], range: Option<std::ops::Range<usize>>, row: u32) -> String {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), bytes).unwrap();
    let buffer = Buffer::from_reader_detecting(bytes).unwrap();
    let file = extract(dir.path(), Path::new("a.rs")).unwrap();
    let symbol = file
        .symbols
        .iter()
        .find(|symbol| symbol.name == "target")
        .expect("target is extracted");
    assert_eq!(
        buffer.text().slice_to_string(symbol.name_range.clone()),
        "target"
    );
    if let Some(range) = range {
        assert_eq!(symbol.name_range, range);
    }
    assert_eq!(symbol.row, row);
    assert_eq!(
        buffer.text().offset_to_point(symbol.name_range.start).row,
        row
    );
    buffer.encoding().label()
}

#[test]
fn extracted_symbols_in_a_bom_file_select_their_names() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"// heading\nfn target() {}\n");
    check_target(&bytes, Some(14..20), 1);
}

#[test]
fn extracted_symbols_in_a_windows_1252_file_select_their_names() {
    let bytes = b"// caf\xE9 na\xEFve\r\nfn target() {}\r\n";
    let label = check_target(bytes, None, 1);
    assert_eq!(label, "windows-1252");
}
