//! A commit made at the path itself (`CommitOptions::at_the_path`, a Save
//! As destination) never goes through a symlink there, even one to nothing,
//! however late it appears (docs/decisions/h6ngbbm2jd-fe-host.md).

#![cfg(unix)]

use std::path::Path;

use fe_host::{CommitOptions, LocalFs, WriteError, WriteExpect};

fn refused(link: &Path, options: CommitOptions<'_>) {
    let result = LocalFs.write_atomic_with(link, b"mine", WriteExpect::Absent, options);
    assert!(matches!(result, Err(WriteError::Conflict(_))), "{result:?}");
}

fn kept(dir: &Path, link: &Path, referent: &Path) {
    assert_eq!(std::fs::read_link(link).unwrap(), referent, "link kept");
    assert!(!referent.exists(), "nothing written through the link");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1, "temp file gone");
}

#[test]
fn at_the_path_refuses_a_dangling_link_there_at_the_start() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    refused(&link, CommitOptions::default().at_the_path());
    kept(dir.path(), &link, &referent);
}

#[test]
fn at_the_path_refuses_a_dangling_link_made_before_the_recheck() {
    let dir = tempfile::tempdir().unwrap();
    let (link, referent) = (dir.path().join("link.txt"), dir.path().join("referent.txt"));
    let options = CommitOptions::default()
        .at_the_path()
        .before_commit(|| std::os::unix::fs::symlink(&referent, &link).unwrap());
    refused(&link, options);
    kept(dir.path(), &link, &referent);
}
