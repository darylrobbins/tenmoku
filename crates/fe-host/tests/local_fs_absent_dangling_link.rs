//! `WriteExpect::Absent` follows a symlink to nothing that is at the path
//! when the write starts, and creates the file it names without clobbering
//! (docs/decisions/h6ngbbm2jd-fe-host.md).

#![cfg(unix)]

use fe_host::{CommitOptions, FsHost as _, LocalFs, WriteError, WriteExpect};

#[test]
fn absent_follows_a_dangling_link_to_create_its_referent() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    LocalFs
        .write_atomic(&link, b"mine", WriteExpect::Absent)
        .unwrap();
    assert_eq!(std::fs::read_link(&link).unwrap(), referent, "link kept");
    assert_eq!(std::fs::read_to_string(&referent).unwrap(), "mine");
}

#[test]
fn absent_through_a_dangling_link_keeps_a_referent_created_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    let hooks =
        CommitOptions::default().before_persist(|| std::fs::write(&referent, "theirs").unwrap());
    let result = LocalFs.write_atomic_with(&link, b"mine", WriteExpect::Absent, hooks);
    assert!(matches!(result, Err(WriteError::Conflict(_))), "{result:?}");
    assert_eq!(std::fs::read_to_string(&referent).unwrap(), "theirs");
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        2,
        "temp file gone"
    );
}
