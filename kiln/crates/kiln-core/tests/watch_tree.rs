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

/// Failure-only bound: the 3 s re-stat plus event latency under heavy load.
/// A batch is returned on as soon as it arrives, so this is never waited out
/// on the passing path.
const WITHIN: Duration = Duration::from_secs(60);

/// Waits up to [`WITHIN`] for a batch naming `root`; on failure, says which
/// batches came instead.
fn reported(batches: &mpsc::Receiver<Vec<PathBuf>>, root: &PathBuf) -> Result<(), String> {
    let started = Instant::now();
    let deadline = started + WITHIN;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match batches.recv_timeout(left) {
            Ok(batch) if batch.contains(root) => return Ok(()),
            Ok(batch) => seen.push(batch),
            Err(_) => break,
        }
    }
    Err(format!(
        "no batch named {root:?} within {:?}; batches seen without it: {seen:?}",
        started.elapsed()
    ))
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

    let dir = root.join("acme.solarized");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("extension.toml"), "id = \"acme.solarized\"\n").unwrap();
    reported(&batches, &root).unwrap();

    settle(&batches, Duration::from_millis(200));
    fs::create_dir_all(late.join("acme.late")).unwrap();
    reported(&batches, &late).unwrap_or_else(|why| panic!("the late root: {why}"));

    // Watched from then on.
    settle(&batches, Duration::from_millis(200));
    fs::write(late.join("acme.late").join("extension.toml"), "").unwrap();
    reported(&batches, &late).unwrap_or_else(|why| panic!("the late root, edited: {why}"));
}
