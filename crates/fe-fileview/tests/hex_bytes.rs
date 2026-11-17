//! A hex view's reads: raw bytes by range, and finding any bytes.

use std::fs::{self, OpenOptions};
use std::sync::atomic::AtomicBool;

use fe_fileview::{
    CHUNK_BYTES, Direction::Backward, Direction::Forward, FIND_BLOCK, FileView, FindOutcome,
    ReadError, find_bytes,
};

/// A file of `bytes`, and a view of it.
fn file_of(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf, FileView) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    fs::write(&path, bytes).unwrap();
    let view = FileView::open(&path).unwrap();
    (dir, path, view)
}

fn found(start: u64, len: u64, wrapped: bool) -> FindOutcome {
    FindOutcome::Found {
        range: start..start + len,
        wrapped,
    }
}

#[test]
fn bytes_reads_raw_bytes_across_a_chunk_edge() {
    let len = 2 * CHUNK_BYTES as usize + 100;
    let mut bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    let edge = CHUNK_BYTES;
    // A NUL and a byte that isn't UTF-8, either side of the edge.
    bytes[edge as usize - 1] = 0;
    bytes[edge as usize] = 0xFF;
    let (_dir, _, view) = file_of(&bytes);

    let read = view.bytes(edge - 10..edge + 10).unwrap();
    assert_eq!(read, bytes[edge as usize - 10..edge as usize + 10]);
    // Invalid UTF-8 and NULs come back as they are, not decoded.
    assert!(read.contains(&0));
    assert!(read.iter().any(|&b| b >= 0x80));

    // Clamped to the length: past the end is short, then empty.
    let tail = view.bytes(len as u64 - 4..len as u64 + 100).unwrap();
    assert_eq!(tail, bytes[len - 4..]);
    assert_eq!(
        view.bytes(len as u64 + 5..len as u64 + 9).unwrap(),
        Vec::<u8>::new()
    );
    // A range that ends before it starts is empty.
    let (start, end) = (7, 3);
    assert_eq!(view.bytes(start..end).unwrap(), Vec::<u8>::new());
    assert_eq!(view.bytes(0..len as u64).unwrap(), bytes);
}

#[test]
fn bytes_past_a_truncation_is_changed() {
    let bytes = vec![0xAB; 3 * CHUNK_BYTES as usize];
    let (_dir, path, view) = file_of(&bytes);
    // Cached before the cut, so still read after it.
    assert_eq!(view.bytes(0..16).unwrap(), [0xAB; 16]);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(CHUNK_BYTES / 2)
        .unwrap();

    assert_eq!(view.bytes(0..16).unwrap(), [0xAB; 16]);
    let past = view.bytes(2 * CHUNK_BYTES..2 * CHUNK_BYTES + 16);
    assert!(matches!(past, Err(ReadError::Changed)), "{past:?}");
}

#[test]
fn find_bytes_finds_nuls_and_newlines() {
    let mut bytes = vec![b'.'; 3 * FIND_BLOCK as usize];
    let needle = [0xDE, 0x00, 0x0A, 0x0D, 0xEF];
    // Once early, once across a find block's edge.
    let edge = FIND_BLOCK as usize - 2;
    for at in [100, edge] {
        bytes[at..at + needle.len()].copy_from_slice(&needle);
    }
    bytes[5000..5003].copy_from_slice(b"AbC");
    let (_dir, _, view) = file_of(&bytes);
    let all = 0..bytes.len() as u64;
    let go = |needle: &[u8], case: bool, from: u64, direction| {
        let stop = AtomicBool::new(false);
        find_bytes(&view, needle, case, all.clone(), from, direction, &stop).unwrap()
    };

    assert_eq!(go(&needle, true, 0, Forward), found(100, 5, false));
    assert_eq!(
        go(&needle, true, 101, Forward),
        found(edge as u64, 5, false)
    );
    assert_eq!(
        go(&needle, true, all.end, Backward),
        found(edge as u64, 5, false)
    );
    assert_eq!(go(&[0x0A, 0x0D], true, 0, Forward), found(102, 2, false));
    // Text, with and without case.
    assert_eq!(go(b"abc", true, 0, Forward), FindOutcome::NotFound);
    assert_eq!(go(b"abc", false, 0, Forward), found(5000, 3, false));
    assert_eq!(go(b"", true, 0, Forward), FindOutcome::NotFound);

    // A set stop flag ends the scan before it reads.
    let stop = AtomicBool::new(true);
    let stopped = find_bytes(&view, &needle, true, all.clone(), 0, Forward, &stop).unwrap();
    assert_eq!(stopped, FindOutcome::Stopped);
}

#[test]
fn find_bytes_wraps_both_ways() {
    let mut bytes = vec![0u8; 4096];
    bytes[1000..1002].copy_from_slice(&[0xFF, 0x00]);
    bytes[3000..3002].copy_from_slice(&[0xFF, 0x00]);
    let (_dir, _, view) = file_of(&bytes);
    let all = 0..bytes.len() as u64;
    let go = |from: u64, direction| {
        let stop = AtomicBool::new(false);
        find_bytes(
            &view,
            &[0xFF, 0x00],
            true,
            all.clone(),
            from,
            direction,
            &stop,
        )
        .unwrap()
    };

    assert_eq!(go(1001, Forward), found(3000, 2, false));
    assert_eq!(go(3001, Forward), found(1000, 2, true));
    assert_eq!(go(3000, Backward), found(1000, 2, false));
    assert_eq!(go(1000, Backward), found(3000, 2, true));
    // Only one match: found again from past it, on the wrap.
    let (_dir, _, one) = file_of(&[0x00, 0x00, 0x7F, 0x00]);
    let stop = AtomicBool::new(false);
    assert_eq!(
        find_bytes(&one, &[0x7F], true, 0..4, 3, Forward, &stop).unwrap(),
        found(2, 1, true)
    );
    assert_eq!(
        find_bytes(&one, &[0x42], true, 0..4, 0, Forward, &stop).unwrap(),
        FindOutcome::NotFound
    );
}
