// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`TreeWatcher`] on real directories: a change under a root is reported
//! by an event, or by the re-stat every 3 s if no event comes.

use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use kiln_core::TreeWatcher;

/// The 3 s re-stat, plus margin.
const WITHIN: Duration = Duration::from_secs(5);

/// Waits up to [`WITHIN`] for a batch naming `root`.
fn reported(batches: &mpsc::Receiver<Vec<PathBuf>>, root: &PathBuf) -> bool {
    let deadline = Instant::now() + WITHIN;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match batches.recv_timeout(left) {
            Ok(batch) if batch.contains(root) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

/// Drains the batches already sent, and those that come within `quiet`.
fn settle(batches: &mpsc::Receiver<Vec<PathBuf>>, quiet: Duration) {
    while batches.recv_timeout(quiet).is_ok() {}
}

#[test]
fn a_tree_watcher_reports_a_new_extension_dir() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    fs::create_dir_all(&root).unwrap();
    let late = state.path().join("later");
    let (sender, batches) = mpsc::channel();
    let _watcher = TreeWatcher::new(vec![root.clone(), late.clone()], move |batch| {
        let _ = sender.send(batch);
    })
    .unwrap();
    // The first batch names every root there is.
    assert_eq!(
        batches.recv_timeout(WITHIN).unwrap(),
        std::slice::from_ref(&root)
    );
    settle(&batches, Duration::from_millis(200));

    let started = Instant::now();
    let dir = root.join("acme.solarized");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("extension.toml"), "id = \"acme.solarized\"\n").unwrap();
    assert!(
        reported(&batches, &root),
        "not reported after {:?}",
        started.elapsed()
    );

    settle(&batches, Duration::from_millis(200));
    let started = Instant::now();
    fs::create_dir_all(late.join("acme.late")).unwrap();
    assert!(
        reported(&batches, &late),
        "the late root, not reported after {:?}",
        started.elapsed()
    );

    // Watched from then on.
    settle(&batches, Duration::from_millis(200));
    fs::write(late.join("acme.late").join("extension.toml"), "").unwrap();
    assert!(reported(&batches, &late));
}
