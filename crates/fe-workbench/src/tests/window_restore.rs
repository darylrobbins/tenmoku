//! A folder launch restores the last session's windows, not every entry
//! ever saved (T1259).

use std::path::Path;

use gpui::TestAppContext;
use kiln_shell::layout::{LayoutStore, WindowLayout, launch};
use serde_json::{Value, json};

fn store(dir: &tempfile::TempDir) -> LayoutStore {
    LayoutStore::new(dir.path(), Path::new("/restore-root"))
}

fn write_file(store: &LayoutStore, entries: Vec<Value>) {
    std::fs::create_dir_all(store.file().parent().unwrap()).unwrap();
    let text = json!({"version": 1, "windows": entries});
    std::fs::write(store.file(), serde_json::to_vec(&text).unwrap()).unwrap();
}

fn entry(id: u64, saved: u64, session: Option<u64>) -> Value {
    let mut entry = json!({"id": id, "saved": saved});
    if let Some(session) = session {
        entry["session"] = json!(session);
    }
    entry
}

fn ids(windows: &[(WindowLayout, Option<Value>)]) -> Vec<u64> {
    let mut ids: Vec<u64> = windows.iter().filter_map(|(w, _)| w.id()).collect();
    ids.sort();
    ids
}

fn entry_ids(store: &LayoutStore) -> Vec<u64> {
    let mut ids: Vec<u64> = (store.entries().iter())
        .filter_map(|entry| entry["id"].as_u64())
        .collect();
    ids.sort();
    ids
}

#[gpui::test]
fn a_folder_launch_restores_only_the_last_session(cx: &mut TestAppContext) {
    // Last session only; older sessions pruned; crash recovery.
    let dir = tempfile::tempdir().unwrap();
    let a = store(&dir);
    write_file(
        &a,
        vec![
            entry(1, 1000, Some(7)),
            entry(2, 2000, Some(9)),
            entry(3, 2001, Some(9)),
        ],
    );
    let windows = launch(&a, true);
    assert_eq!(ids(&windows), [2, 3]);
    drop(windows);
    assert_eq!(entry_ids(&a), [2, 3]);
    let windows = launch(&a, true);
    assert_eq!(ids(&windows), [2, 3]);
    drop(windows);

    // At most 8 legacy entries.
    let dir = tempfile::tempdir().unwrap();
    let b = store(&dir);
    write_file(&b, (100..120).map(|i| entry(i, 1000 + i, None)).collect());
    let windows = launch(&b, true);
    assert_eq!(ids(&windows), (112..=119).collect::<Vec<u64>>());
    drop(windows);
    assert_eq!(b.entries().len(), 8);

    // A file launch's window is never restored.
    let dir = tempfile::tempdir().unwrap();
    let c = store(&dir);
    let mut file_launch = launch(&c, false);
    let (layout, _) = file_launch.pop().unwrap();
    let file_id = layout.id().unwrap();
    cx.update(|cx| layout.close(json!({"panes": {}}), false, cx));
    cx.run_until_parked();
    assert_eq!(entry_ids(&c), [file_id]);
    let windows = launch(&c, true);
    assert_eq!(windows.len(), 1);
    assert!(windows[0].1.is_none());
    assert_ne!(windows[0].0.id(), Some(file_id));
    drop(windows);

    // A live window's entry is untouched.
    let dir = tempfile::tempdir().unwrap();
    let d = store(&dir);
    write_file(&d, vec![entry(5, 1000, Some(7)), entry(6, 2000, Some(9))]);
    let live = d.claim(5).unwrap().unwrap();
    let windows = launch(&d, true);
    assert_eq!(ids(&windows), [6]);
    assert!(entry_ids(&d).contains(&5));
    drop(windows);
    drop(live);
}
