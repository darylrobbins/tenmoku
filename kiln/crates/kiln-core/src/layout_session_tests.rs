// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::json;

fn store(dir: &tempfile::TempDir) -> LayoutStore {
    LayoutStore::new(dir.path(), Path::new("/root"))
}

#[test]
fn a_written_entry_carries_this_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let claim = store.claim_new().unwrap();
    store.write(&claim, json!({})).unwrap();
    assert_eq!(entry_session(&store.entries()[0]), Some(session()));
}

#[test]
fn a_live_claim_keeps_its_older_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let other = store.claim(5).unwrap().unwrap();
    store
        .update(5, Some(json!({"id": 5, "saved": 1, "session": 1})))
        .unwrap();
    store
        .update(6, Some(json!({"id": 6, "saved": 2, "session": 2})))
        .unwrap();
    let last = store.restore_last_session(8);
    assert_eq!(last.windows.len(), 1);
    assert_eq!(last.dropped, 0);
    let ids: Vec<_> = store.entries().iter().filter_map(entry_id).collect();
    assert_eq!(ids.len(), 2);
    drop(other);
}

/// Writes `entries` to `store`'s file as a write would, its root recorded.
fn put(store: &LayoutStore, entries: &[(u64, u64, u64)]) {
    for (id, saved, session) in entries {
        store
            .update(
                *id,
                Some(json!({"id": id, "saved": saved, "session": session})),
            )
            .unwrap();
    }
}

#[test]
fn a_write_records_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let claim = store.claim_new().unwrap();
    store.write(&claim, json!({})).unwrap();
    assert_eq!(read(&store.file()).0.as_deref(), Some(Path::new("/root")));
    let rootless = LayoutStore::new(dir.path(), Path::new(""));
    put(&rootless, &[(1, 1, 1)]);
    assert_eq!(read(&rootless.file()).0.as_deref(), Some(Path::new("")));
}

/// The ids `windows` restore, in order.
fn ids(windows: &[QuitWindow]) -> Vec<u64> {
    (windows.iter())
        .filter_map(|window| entry_id(&window.entry))
        .collect()
}

fn stored(store: &LayoutStore) -> Vec<u64> {
    let mut ids: Vec<_> = store.entries().iter().filter_map(entry_id).collect();
    ids.sort();
    ids
}

#[test]
fn the_last_quit_spans_roots() {
    let dir = tempfile::tempdir().unwrap();
    let at = |root: &str| LayoutStore::new(dir.path(), Path::new(root));
    let (a, b, rootless, old) = (at("/a"), at("/b"), at(""), at("/old"));
    put(&a, &[(1, 30, 9), (2, 10, 7)]);
    put(&b, &[(3, 40, 9)]);
    put(&rootless, &[(4, 20, 9)]);
    put(&old, &[(5, 5, 7)]);
    // A file launch's newer window is not the last session's.
    let entry = json!({"id": 6, "saved": 50, "session": 11, "restore": false});
    old.update(6, Some(entry)).unwrap();
    // Nor is a file that records no root and shows none, however new.
    let legacy = at("/legacy");
    fs::write(
        legacy.file(),
        r#"{"version": 1, "windows": [{"id": 7, "saved": 99, "session": 12, "path": "x"}]}"#,
    )
    .unwrap();
    let windows = restore_last_quit(dir.path(), 8, |_| true);
    assert_eq!(ids(&windows), [3, 1, 4], "newest first");
    let roots: Vec<_> = windows.iter().map(|w| w.root.clone()).collect();
    assert_eq!(roots, ["/b", "/a", ""].map(PathBuf::from));
    // The restored stores' older sessions go; the others are untouched.
    assert_eq!(stored(&a), [1]);
    assert_eq!(stored(&old), [5, 6]);
    assert_eq!(stored(&legacy), [7]);
    for window in &windows {
        assert_eq!(entry_session(&window.store.entries()[0]), Some(session()));
    }

    // Held, they're live windows, not restored again: the newest left is
    // the older session's.
    let again = restore_last_quit(dir.path(), 8, |_| true);
    assert_eq!(ids(&again), [5]);
    drop((windows, again));
    let empty = tempfile::tempdir().unwrap();
    assert!(restore_last_quit(empty.path(), 8, |_| true).is_empty());
    assert!(restore_last_quit(&empty.path().join("missing"), 8, |_| true).is_empty());
}

/// A window re-rooted since it opened reopens on its new folder, in its
/// store; one whose folder is gone is skipped and kept.
#[test]
fn a_window_reopens_on_the_folder_it_was_on() {
    let dir = tempfile::tempdir().unwrap();
    let rootless = LayoutStore::new(dir.path(), Path::new(""));
    let a = LayoutStore::new(dir.path(), Path::new("/a"));
    let entry = json!({"id": 1, "saved": 30, "session": 9, "root": "/b"});
    rootless.update(1, Some(entry)).unwrap();
    let entry = json!({"id": 2, "saved": 20, "session": 9, "root": "/gone"});
    a.update(2, Some(entry)).unwrap();
    let windows = restore_last_quit(dir.path(), 8, |root| root != Path::new("/gone"));
    assert_eq!(ids(&windows), [1]);
    assert_eq!(windows[0].root, Path::new("/b"));
    assert_eq!(windows[0].store, rootless);
    assert_eq!(stored(&a), [2], "the gone folder's window is kept");
}

/// The cap takes the newest windows of all; the rest are left as they are.
#[test]
fn the_last_quit_is_capped() {
    let dir = tempfile::tempdir().unwrap();
    let at = |root: &str| LayoutStore::new(dir.path(), Path::new(root));
    let (a, b) = (at("/a"), at("/b"));
    put(&a, &[(1, 10, 9), (2, 30, 9), (3, 50, 9)]);
    put(&b, &[(4, 20, 9), (5, 40, 9)]);
    let windows = restore_last_quit(dir.path(), 3, |_| true);
    assert_eq!(ids(&windows), [3, 5, 2]);
    drop(windows);
    assert_eq!((stored(&a), stored(&b)), (vec![1, 2, 3], vec![4, 5]));
}

/// A store whose last-session window another process holds, with an older
/// session's beside it, is not one of the last quit's: the older window
/// isn't restored in its place, and stays.
#[test]
fn a_held_window_leaves_its_stores_older_session_alone() {
    let dir = tempfile::tempdir().unwrap();
    let at = |root: &str| LayoutStore::new(dir.path(), Path::new(root));
    let (a, b) = (at("/a"), at("/b"));
    put(&a, &[(1, 50, 9), (2, 10, 7)]);
    put(&b, &[(3, 40, 9)]);
    let held = a.claim(1).unwrap().unwrap();
    let windows = restore_last_quit(dir.path(), 8, |_| true);
    assert_eq!(ids(&windows), [3]);
    drop(windows);
    assert_eq!(stored(&a), [1, 2]);
    drop(held);
}

#[test]
fn a_file_from_before_roots_were_recorded_is_found_by_its_paths() {
    let dir = tempfile::tempdir().unwrap();
    let write = |store: &LayoutStore, entry: &str| {
        let text = format!(r#"{{"version": 1, "windows": [{entry}]}}"#);
        fs::write(store.file(), text).unwrap();
    };
    fs::create_dir_all(dir.path()).unwrap();
    // Absolute on every OS; it need not exist.
    let old_root = std::env::temp_dir().join("old").join("project");
    let old_file = old_root.join("src").join("main.rs");
    let a = LayoutStore::new(dir.path(), &old_root);
    // A JSON string literal, quotes off: a Windows path needs its backslashes escaped.
    let old_file_json = serde_json::to_string(&old_file.to_string_lossy()).unwrap();
    let old_file_json = &old_file_json[1..old_file_json.len() - 1];
    write(
        &a,
        &format!(
            r#"{{"id": 1, "saved": 5, "session": 3, "panes": {{"items": [{{"state": {{"path": "{old_file_json}"}}}}]}}}}"#
        ),
    );
    let rootless = LayoutStore::new(dir.path(), Path::new(""));
    write(&rootless, r#"{"id": 2, "saved": 4, "session": 3}"#);
    let found = restore_last_quit(dir.path(), 8, |_| true);
    let roots: Vec<_> = found.iter().map(|w| w.root.clone()).collect();
    assert_eq!(roots, [old_root, PathBuf::new()]);
    assert_eq!(found[0].store, a);
    assert_eq!(found[1].store, rootless);
}
