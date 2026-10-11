// SPDX-License-Identifier: LicenseRef-Proprietary

//! Saved window layouts: a stable key per root, writes that merge each
//! window's entry, files of another version or gone bad read as empty,
//! and claims that give each window id one live owner, across processes
//! (two stores on one directory stand in for two processes: each claim is
//! its own lock).

use std::{fs, path::Path};

use kiln_command::Value;

use crate::layout::{LayoutStore, entry_id, layout_key};

fn entry(name: &str) -> Value {
    Value::from_iter([("name", Value::from(name))])
}

fn names(store: &LayoutStore) -> Vec<String> {
    let mut names: Vec<String> = (store.entries().iter())
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_key_is_stable_and_per_root() {
    assert_eq!(layout_key(Path::new("/a/b")), layout_key(Path::new("/a/b")));
    assert_ne!(layout_key(Path::new("/a/b")), layout_key(Path::new("/a/c")));
    // FNV-1a 64 of the bytes, in hex.
    assert_eq!(layout_key(Path::new("a")), "af63dc4c8601ec8c");
}

#[test]
fn writes_merge_and_deletes_take_only_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let store = LayoutStore::new(dir.path(), Path::new("/project"));
    let (a, b) = (store.claim_new().unwrap(), store.claim_new().unwrap());
    assert_ne!(a.id(), b.id());
    store.write(&a, entry("a")).unwrap();
    store.write(&b, entry("b")).unwrap();
    store.write(&a, entry("a2")).unwrap();
    assert_eq!(names(&store), ["a2", "b"]);
    // Newest first.
    assert_eq!(entry_id(&store.entries()[0]), Some(a.id()));
    store.delete(&b).unwrap();
    assert_eq!(names(&store), ["a2"]);

    // Another root's file is its own.
    let other = LayoutStore::new(dir.path(), Path::new("/elsewhere"));
    assert_eq!(names(&other), Vec::<String>::new());
}

#[test]
fn another_version_or_a_bad_file_reads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = LayoutStore::new(dir.path(), Path::new("/project"));
    fs::write(store.file(), r#"{"version": 2, "windows": [{"id": 1}]}"#).unwrap();
    assert_eq!(store.entries(), Vec::<Value>::new());
    fs::write(store.file(), "not json").unwrap();
    assert_eq!(store.entries(), Vec::<Value>::new());
    let claim = store.claim_new().unwrap();
    store.write(&claim, entry("a")).unwrap();
    assert_eq!(names(&store), ["a"]);
}

#[test]
fn a_claimed_id_has_one_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = LayoutStore::new(dir.path(), Path::new("/project"));
    let claim = store.claim_new().unwrap();
    store.write(&claim, entry("a")).unwrap();
    assert!(store.claim(claim.id()).unwrap().is_none());
    assert!(store.restorable().is_empty());
    let id = claim.id();
    drop(claim);
    let (again, saved) = store.newest_restorable().unwrap();
    assert_eq!((again.id(), saved["name"].as_str()), (id, Some("a")));
}

/// F4's two processes on one root: A restores its entries and writes; B
/// launches the same root while A runs and gets fresh ids, not A's
/// windows; B's close leaves A's entries; A's later writes and close
/// touch only A's; once A dies, a third launch restores A's entries.
#[test]
fn two_processes_on_one_root_keep_to_their_own_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new("/project");
    let first = LayoutStore::new(dir.path(), root);
    let (one, two) = (first.claim_new().unwrap(), first.claim_new().unwrap());
    first.write(&one, entry("a1")).unwrap();
    first.write(&two, entry("a2")).unwrap();
    let ids = [one.id(), two.id()];
    drop((one, two));

    // A launches and restores both.
    let a = LayoutStore::new(dir.path(), root);
    let a_windows = a.restorable();
    let mut restored: Vec<u64> = a_windows.iter().map(|(claim, _)| claim.id()).collect();
    restored.sort();
    let mut expected = ids.to_vec();
    expected.sort();
    assert_eq!(restored, expected);
    // Newest first: a2 was written last.
    assert_eq!(a_windows[0].1["name"].as_str(), Some("a2"));
    a.write(&a_windows[1].0, entry("a1")).unwrap();

    // B launches while A runs: nothing to restore, so a fresh window.
    let b = LayoutStore::new(dir.path(), root);
    assert!(b.restorable().is_empty());
    let b_window = b.claim_new().unwrap();
    assert!(!ids.contains(&b_window.id()));
    b.write(&b_window, entry("b")).unwrap();
    assert_eq!(names(&a), ["a1", "a2", "b"]);

    // B's window closes while A's stay: only its entry goes.
    b.delete(&b_window).unwrap();
    b_window.release(true);
    assert_eq!(names(&a), ["a1", "a2"]);

    // A writes and closes one window, deleting its entry.
    a.write(&a_windows[0].0, entry("a2 again")).unwrap();
    let (closing, _) = &a_windows[1];
    a.delete(closing).unwrap();
    assert_eq!(names(&b), ["a2 again"]);

    // A dies: its claims go with it, and a third launch restores.
    drop(a_windows);
    let third = LayoutStore::new(dir.path(), root);
    let restored = third.restorable();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].1["name"].as_str(), Some("a2 again"));
}
