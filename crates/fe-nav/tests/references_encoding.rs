//! `SearchNavigation::references` in files the editor decodes: with a byte
//! order mark, in a legacy encoding, in UTF-16. Each reference selects the
//! word in the text the editor loads.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
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

/// A file of a legacy-encoded comment, then one line of `units` hits: a
/// `target` every 1.5 KB.
fn long_legacy_line(units: usize) -> Vec<u8> {
    let mut bytes = b"// caf\xE9\n".to_vec();
    let unit = [&b"target "[..], &[b'a'; 1_500][..], b"\xE9 "].concat();
    for _ in 0..units {
        bytes.extend(&unit);
    }
    bytes.push(b'\n');
    bytes
}

/// The references found in `file.rs` once `bytes` are written there, the
/// least time of `runs` searches, and whether a search was cancelled for
/// running past `limit`.
fn timed_find(
    bytes: &[u8],
    runs: usize,
    limit: Option<std::time::Duration>,
) -> (Vec<Reference>, std::time::Duration, bool) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.rs"), bytes).unwrap();
    let mut least = std::time::Duration::MAX;
    let mut batches = Vec::new();
    let mut cancelled = false;
    for _ in 0..runs {
        let cancel = AtomicBool::new(false);
        let done = AtomicBool::new(false);
        let start = std::time::Instant::now();
        std::thread::scope(|scope| {
            // A search that is quadratic runs for minutes: stop it at the limit.
            scope.spawn(|| {
                while !done.load(Ordering::Relaxed) {
                    if limit.is_some_and(|limit| start.elapsed() > limit) {
                        cancel.store(true, Ordering::Relaxed);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            });
            // Set on drop, so a panic below still stops the watchdog.
            struct Done<'a>(&'a AtomicBool);
            impl Drop for Done<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _done = Done(&done);
            batches = Vec::new();
            search(dir.path())
                .references(&request(dir.path()), &cancel, &mut |batch| {
                    batches.push(batch)
                })
                .unwrap();
        });
        cancelled |= cancel.load(Ordering::Relaxed);
        least = least.min(start.elapsed());
        if cancelled {
            break;
        }
    }
    let refs = (batches.into_iter().flatten())
        .filter(|file| file.rel == Path::new("file.rs"))
        .flat_map(|file| file.refs)
        .collect();
    (refs, least, cancelled)
}

/// A long line in a legacy encoding with many hits is decoded about once,
/// not from its start for each hit. The check is on how the time grows with
/// the line, not on the time, so machine load doesn't decide it: the least
/// of a few runs of a line eight times as long takes about eight times as
/// long if the line is decoded once (64 if it's decoded per hit). The
/// baseline is long enough (about 0.7 s, mostly fixed cost) that load doesn't swamp it, and
/// a search that outruns the bound is cancelled, so a quadratic one fails
/// in seconds rather than minutes.
#[test]
fn a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work() {
    // One 12 MB line, and one an eighth as long.
    let bytes = long_legacy_line(8_000);
    let (eighth_refs, eighth, _) = timed_find(&long_legacy_line(1_000), 5, None);
    // Three times the slack over the 8 of linear growth, well short of the 64
    // of quadratic.
    let (refs, long, cancelled) = timed_find(&bytes, 3, Some(eighth * 24));
    assert!(
        !cancelled,
        "8x the line took over 24x as long (the eighth took {eighth:?})"
    );
    let buffer = Buffer::from_reader_detecting(bytes.as_slice()).unwrap();
    assert_ne!(buffer.encoding().name(), "UTF-8");
    assert!(refs.len() >= 8_000, "{}", refs.len());
    assert!(eighth_refs.len() >= 1_000, "{}", eighth_refs.len());
    for r in &refs {
        let text = buffer.text().slice_to_string(r.location.range.clone());
        assert_eq!(text, "target", "{r:?}");
    }
    assert!(
        long < eighth * 24,
        "8x the line took {:.1}x as long ({eighth:?}, {long:?})",
        long.as_secs_f64() / eighth.as_secs_f64()
    );
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
