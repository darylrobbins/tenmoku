//! [`LocalSearch::files`]'s stream cancels its walk when dropped: in
//! batches of one path, so a small tree outruns what the walk and the
//! stream hold for a reader that stopped.

use std::fs;

use fe_host::SearchFilter;

use super::{STREAMED, files_in};
use crate::finder::{QUEUED, Walked, walk_threads};

/// More files than a stalled walk lists in batches of one: the reader's
/// batch, the stream's, the one blocked sending, the walk's queue, and a
/// batch blocked and one being filled per thread.
fn more_than_held() -> usize {
    1 + STREAMED + 1 + QUEUED + 2 * walk_threads() + 50
}

#[test]
fn dropping_the_file_list_ends_its_walk_unfinished() {
    let dir = tempfile::tempdir().unwrap();
    let total = more_than_held();
    for index in 0..total {
        fs::write(dir.path().join(format!("f{index:04}")), "").unwrap();
    }
    let stream = files_in(dir.path(), &SearchFilter::default(), Walked::with_batch(1));
    let ended = stream.ended();
    assert_eq!(stream.recv().map(|batch| batch.len()), Some(1));
    drop(stream);
    let summary = ended.wait().expect("an end value").unwrap();
    assert!(!summary.complete, "the walk ran on after the drop");
}

#[test]
fn the_file_list_read_to_the_end_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..10 {
        fs::write(dir.path().join(format!("f{index:04}")), "").unwrap();
    }
    let mut stream = files_in(dir.path(), &SearchFilter::default(), Walked::with_batch(1));
    let listed: usize = stream.by_ref().map(|batch| batch.len()).sum();
    assert_eq!(listed, 10);
    assert!(stream.finish().unwrap().unwrap().complete);
}
