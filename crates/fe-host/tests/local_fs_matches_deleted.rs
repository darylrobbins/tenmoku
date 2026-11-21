//! `WriteExpect::Matches` refuses a file deleted since its stamp was taken,
//! unless the local commit is told to write one
//! (`CommitOptions::deleted_is_written`, an editor's save).

use fe_host::{CommitOptions, FsHost as _, LocalFs, WriteError, WriteExpect};

#[test]
fn matches_refuses_a_file_deleted_since() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "old").unwrap();
    let (_, stamp) = LocalFs.read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let result = LocalFs.write_atomic(&path, b"mine", WriteExpect::Matches(stamp));
    assert!(
        matches!(result, Err(WriteError::Conflict(ref conflict)) if conflict.current.is_none()),
        "{result:?}"
    );
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "nothing written"
    );
}

#[test]
fn matches_writes_a_deleted_file_when_told_to() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "old").unwrap();
    let (_, stamp) = LocalFs.read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let options = CommitOptions::default().deleted_is_written();
    LocalFs
        .write_atomic_with(&path, b"mine", WriteExpect::Matches(stamp), options)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");
}

#[test]
fn matches_told_to_write_a_deleted_file_keeps_one_made_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "old").unwrap();
    let (_, stamp) = LocalFs.read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let options = CommitOptions::default()
        .deleted_is_written()
        .before_persist(|| std::fs::write(&path, "theirs").unwrap());
    let result = LocalFs.write_atomic_with(&path, b"mine", WriteExpect::Matches(stamp), options);
    assert!(matches!(result, Err(WriteError::Conflict(_))), "{result:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "temp file gone"
    );
}
