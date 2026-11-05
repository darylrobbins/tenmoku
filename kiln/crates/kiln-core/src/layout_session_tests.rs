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
