//! `TrustStore::update` serializes writers: a revocation saved while another
//! writer holds the lock for an unrelated decision survives it.

use std::fs::{File, TryLockError};
use std::path::Path;
use std::sync::mpsc::{channel, sync_channel};
use std::thread;

use fe_config::trust::{Trust, TrustStore};

#[test]
fn a_revocation_waiting_on_the_lock_survives_an_unrelated_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("trust.toml");
    let (x, y) = (Path::new("/x"), Path::new("/y"));
    TrustStore::update(&path, |store| store.decide(x, true)).unwrap();

    let (a_loaded, a_loaded_rx) = sync_channel::<()>(0);
    let (a_release, a_release_rx) = channel::<()>();
    let (b_started, b_started_rx) = channel::<()>();
    let (b_applied, b_applied_rx) = channel::<()>();

    let path = &path;
    thread::scope(move |scope| {
        let a = scope.spawn(move || {
            let (a_loaded, a_release_rx) = (a_loaded, a_release_rx);
            TrustStore::update(path, |store| {
                store.decide(y, true);
                a_loaded.send(()).unwrap();
                a_release_rx.recv().unwrap();
            })
            .unwrap();
        });
        a_loaded_rx.recv().unwrap();

        let b = scope.spawn(move || {
            let (b_started, b_applied) = (b_started, b_applied);
            // A holds the lock, so a second open of the lock file, here in
            // the same process, can't take it. That is the contention B's
            // update then waits out.
            let probe = File::open(path.with_file_name("trust.toml.lock")).unwrap();
            assert!(matches!(probe.try_lock(), Err(TryLockError::WouldBlock)));
            drop(probe);
            b_started.send(()).unwrap();
            TrustStore::update(path, |store| {
                b_applied.send(()).unwrap();
                store.decide(x, false);
            })
            .unwrap();
        });
        b_started_rx.recv().unwrap();
        // B has seen the lock held, and can't apply until A lets go: without
        // the lock it would load the file as it was before A saved, and
        // revoke X over A's later save of the stale snapshot.
        assert!(b_applied_rx.try_recv().is_err());
        a_release.send(()).unwrap();
        a.join().unwrap();
        b_applied_rx.recv().unwrap();
        b.join().unwrap();
    });

    let store = TrustStore::load(path);
    assert_eq!(store.trust_of(x), Trust::Untrusted);
    assert_eq!(store.trust_of(y), Trust::Trusted);
}
