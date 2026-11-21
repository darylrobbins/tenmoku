//! `LocalFs::write_atomic` keeps to its `WriteExpect` (D § 7), with each
//! race made at a commit boundary hook.

use std::path::Path;

use fe_host::{CommitOptions, Conflict, DiskStamp, FsHost as _, LocalFs, WriteError, WriteExpect};

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

fn conflict(result: Result<DiskStamp, WriteError>) -> Conflict {
    match result {
        Err(WriteError::Conflict(conflict)) => conflict,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

#[test]
fn absent_refuses_a_file_created_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.txt");
    let hooks =
        CommitOptions::default().before_persist(|| std::fs::write(&path, "theirs").unwrap());
    let result = LocalFs.write_atomic_with(&path, b"mine", WriteExpect::Absent, hooks);
    let current = conflict(result).current;
    assert_eq!(current, DiskStamp::of(&path).unwrap());
    assert!(current.is_some());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
    assert_eq!(entries(dir.path()), 1, "temp file gone");
}

#[test]
fn matches_conflicts_when_the_stamp_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "old").unwrap();
    let (_, stamp) = LocalFs.read(&path).unwrap();
    let hooks =
        CommitOptions::default().before_commit(|| std::fs::write(&path, "theirs!").unwrap());
    let result = LocalFs.write_atomic_with(&path, b"mine", WriteExpect::Matches(stamp), hooks);
    assert_ne!(conflict(result).current, Some(stamp));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs!");
    assert_eq!(entries(dir.path()), 1, "temp file gone");
}

#[test]
fn as_found_on_an_empty_path_does_not_clobber_a_newcomer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.txt");
    let hooks =
        CommitOptions::default().before_persist(|| std::fs::write(&path, "theirs").unwrap());
    let result = LocalFs.write_atomic_with(&path, b"mine", WriteExpect::AsFound, hooks);
    conflict(result);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
    assert_eq!(entries(dir.path()), 1, "temp file gone");
}

#[test]
fn overwrite_replaces_whatever_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "old").unwrap();
    let hooks =
        CommitOptions::default().before_persist(|| std::fs::write(&path, "theirs").unwrap());
    let stamp = LocalFs
        .write_atomic_with(&path, b"mine", WriteExpect::Overwrite, hooks)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");
    assert_eq!(DiskStamp::of(&path).unwrap(), Some(stamp));
    assert_eq!(entries(dir.path()), 1, "temp file gone");
}
