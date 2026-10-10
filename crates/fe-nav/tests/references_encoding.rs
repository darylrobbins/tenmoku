//! `SearchNavigation::references` in files the editor decodes: with a byte
//! order mark, in a legacy encoding, in UTF-16. Each reference selects the
//! word in the text the editor loads.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, atomic::AtomicBool},
};

use fe_buffer::{Buffer, Text};
use fe_nav::{
    FileReferences, NavRequest, NavigationProvider, Reference, ReferenceOptions, SearchNavigation,
    SymbolIndex,
};

fn search(root: &Path) -> SearchNavigation {
    SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))))
        .with_reference_options(ReferenceOptions {
            verify_files: 1_000,
        })
}

/// A request from `req.rs`, whose text mentions `target`.
fn request(root: &Path) -> NavRequest {
    let text = "fn other() { target(); }\n";
    fs::write(root.join("req.rs"), text).unwrap();
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from("req.rs"),
        language: Some("rust"),
        text: Text::new(text),
        offset: text.find("target").unwrap(),
        buffer: None,
    }
}

fn find(root: &Path) -> Vec<Vec<FileReferences>> {
    let mut batches = Vec::new();
    search(root)
        .references(&request(root), &AtomicBool::new(false), &mut |batch| {
            batches.push(batch)
        })
        .unwrap();
    batches
}

/// The references to `target` in `file.rs`, once `bytes` are written there:
/// each selects `target` in the text the editor loads, on the row it's on,
/// and in its preview. Returns the editor's buffer.
fn check(bytes: &[u8]) -> (Buffer, Vec<Reference>) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.rs"), bytes).unwrap();
    let batches = find(dir.path());
    let mut refs: Vec<Reference> = (batches.into_iter().flatten())
        .filter(|file| file.rel == Path::new("file.rs"))
        .flat_map(|file| file.refs)
        .collect();
    refs.sort_by_key(|r| r.location.range.start);
    let buffer = Buffer::from_reader_detecting(bytes).unwrap();
    let text = buffer.text();
    assert_eq!(refs.len(), 2, "{refs:?}");
    for (r, row) in refs.iter().zip([1, 2]) {
        let range = r.location.range.clone();
        assert_eq!(text.slice_to_string(range.clone()), "target", "{r:?}");
        assert_eq!(r.location.row, text.offset_to_point(range.start).row);
        assert_eq!(r.location.row, row);
        assert!(!r.preview_ranges.is_empty(), "{r:?}");
        for p in &r.preview_ranges {
            assert_eq!(r.preview.get(p.clone()), Some("target"), "{r:?}");
        }
    }
    (buffer, refs)
}

const BOM_TEXT: &str = "// heading\nfn target() {}\nfn main() { target(); }\n";

#[test]
fn references_in_a_bom_file_select_the_word() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(BOM_TEXT.as_bytes());
    check(&bytes);
}

#[test]
fn references_in_a_windows_1252_file_select_the_word() {
    let bytes =
        b"// caf\xE9 na\xEFve\r\nfn target() {}\r\nfn main() { let caf\xE9 = target(); }\r\n";
    let (buffer, _) = check(bytes);
    assert_eq!(buffer.encoding().name(), "windows-1252");
}

#[test]
fn references_in_a_utf16_file_select_the_word() {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(BOM_TEXT.encode_utf16().flat_map(u16::to_le_bytes));
    let (_, refs) = check(&bytes);
    assert!(refs.iter().all(|r| r.verified), "{refs:?}");
}

/// A long line in a legacy encoding with many hits is decoded about once,
/// not from its start for each hit.
#[test]
fn a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work() {
    let mut bytes = b"// caf\xE9\n".to_vec();
    // One 3 MB line, `target` every 1.5 KB.
    let unit = [&b"target "[..], &[b'a'; 1_500][..], b"\xE9 "].concat();
    for _ in 0..2_000 {
        bytes.extend(&unit);
    }
    bytes.push(b'\n');
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.rs"), &bytes).unwrap();
    let start = std::time::Instant::now();
    let batches = find(dir.path());
    let elapsed = start.elapsed();
    let buffer = Buffer::from_reader_detecting(bytes.as_slice()).unwrap();
    assert_ne!(buffer.encoding().name(), "UTF-8");
    let refs: Vec<_> = (batches.into_iter().flatten())
        .filter(|file| file.rel == Path::new("file.rs"))
        .flat_map(|file| file.refs)
        .collect();
    assert!(refs.len() >= 2_000, "{}", refs.len());
    for r in &refs {
        let text = buffer.text().slice_to_string(r.location.range.clone());
        assert_eq!(text, "target", "{r:?}");
    }
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
}

/// A hit that starts inside a multibyte character of a legacy encoding is
/// rejected, and the hit after it on the line is still placed.
#[test]
fn a_rejected_hit_inside_a_multibyte_character_leaves_later_hits_placed() {
    // Shift_JIS: `\xC2\x81` is U+0081 as UTF-8 to the search, but in
    // Shift_JIS `\x81t` is one character that a hit boundary splits.
    let mut bytes = b"// \x82\xA0\x82\xA2\x82\xA4\x82\xA6\x82\xA8\n".to_vec();
    bytes.extend(b"fn target() {}\n");
    bytes.extend(b"fn main() { \xC2\x81target target(); }\n");
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.rs"), &bytes).unwrap();
    let buffer = Buffer::from_reader_detecting(bytes.as_slice()).unwrap();
    let batches = find(dir.path());
    let refs: Vec<_> = (batches.into_iter().flatten())
        .filter(|file| file.rel == Path::new("file.rs"))
        .flat_map(|file| file.refs)
        .collect();
    let verified: Vec<_> = refs.iter().filter(|r| r.verified).collect();
    assert!(verified.iter().any(|r| r.location.row == 2), "{refs:?}");
    for r in verified {
        let text = buffer.text().slice_to_string(r.location.range.clone());
        assert_eq!(text, "target", "{r:?}");
    }
}
