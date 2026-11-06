use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::{FinderIndex, FsChange};

/// An index of a project with `a/b.rs`, after `a` is replaced by a file.
fn replaced() -> (tempfile::TempDir, FinderIndex) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("a")).unwrap();
    fs::write(dir.path().join("a/b.rs"), "").unwrap();
    let mut index = FinderIndex::new(dir.path());
    FinderIndex::walk(dir.path(), Arc::new(AtomicBool::new(false)), |batch| {
        index.extend(batch)
    });
    index.finish();
    assert!(index.contains("a/b.rs".replace('/', std::path::MAIN_SEPARATOR_STR)));
    fs::remove_dir_all(dir.path().join("a")).unwrap();
    fs::write(dir.path().join("a"), "").unwrap();
    (dir, index)
}

fn change(index: &FinderIndex, rel: &str) -> FsChange {
    FsChange::Path(index.root_path().join(Path::new(rel)))
}

#[test]
fn file_replacing_a_directory_stays_indexed_when_its_old_child_is_reported() {
    let (_dir, mut index) = replaced();
    index.apply(&[change(&index, "a"), change(&index, "a/b.rs")]);
    assert!(!index.contains("a/b.rs".replace('/', std::path::MAIN_SEPARATOR_STR)));
    assert!(index.contains("a"));
}

#[test]
fn child_reported_alone_under_a_new_file_indexes_the_file() {
    let (_dir, mut index) = replaced();
    index.apply(&[change(&index, "a/b.rs")]);
    assert!(!index.contains("a/b.rs".replace('/', std::path::MAIN_SEPARATOR_STR)));
    assert!(index.contains("a"));
}
