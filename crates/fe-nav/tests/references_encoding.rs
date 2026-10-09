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
