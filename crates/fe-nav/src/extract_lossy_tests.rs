//! The lossily decoded file, parsed without `FULL_PARSE_CAP`'s wall clock so a
//! loaded machine can't turn its slow parse into `Skipped::ParseFailed`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_buffer::Buffer;

use super::extract_with;

#[test]
fn symbols_after_lossily_decoded_bytes_select_their_names() {
    // More than the 64 KiB the loader's detection looks at, so it takes the
    // file for UTF-8 and decodes it lossily. Two invalid bytes after it become
    // two U+FFFDs, of 3 bytes each, so the loaded text's offsets are 4 bytes
    // past the file's.
    let filler = "plain ascii filler\n".repeat(6_000);
    assert!(filler.len() > 64 * 1024);
    let mut lossy = filler.clone().into_bytes();
    lossy.extend(b"// \xff\xfe\nfn after_lossy() {}\n");
    let detected = Buffer::from_reader_detecting(lossy.as_slice()).unwrap();
    assert_eq!(detected.encoding().label(), "UTF-8");

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("lossy.rs"), lossy).unwrap();

    let file = extract_with(&root, Path::new("lossy.rs"), |syntax| {
        assert!(syntax.parse_until(Duration::MAX, || false));
    })
    .unwrap();
    let metadata = fs::metadata(root.join("lossy.rs")).unwrap();
    assert_eq!(file.rel, PathBuf::from("lossy.rs"));
    assert_eq!(file.len, metadata.len());
    assert_eq!(file.modified, Some(metadata.modified().unwrap()));
    assert!(!file.truncated);

    let [symbol] = &file.symbols[..] else {
        panic!("one symbol in {file:?}");
    };
    assert_eq!(symbol.name, "after_lossy");
    assert_eq!(symbol.name_range, filler.len() + 13..filler.len() + 24);
    assert_eq!(symbol.row, 6_001);

    let buffer = Buffer::from_reader(fs::File::open(root.join("lossy.rs")).unwrap()).unwrap();
    assert_eq!(
        buffer.text().slice_to_string(symbol.name_range.clone()),
        "after_lossy"
    );
    assert_eq!(
        buffer.text().offset_to_point(symbol.name_range.start).row,
        symbol.row
    );
    assert!(symbol.range.start <= symbol.name_range.start);
    assert!(symbol.name_range.end <= symbol.range.end);
}
