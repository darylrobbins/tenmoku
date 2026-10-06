//! The finder's walk holds up while its sink is slow: its threads stop
//! listing once a few batches wait, and a cancel, or a sink that panics,
//! ends them promptly.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::*;
use crate::finder::{QUEUED, Walked, walk_root, walk_threads};

/// Paths per batch: small, so a tree of a few thousand files is many
/// batches.
const BATCH: usize = 4;

/// How long a walk may take to end, once its sink goes on, before it counts
/// as stuck: generous, for a loaded machine. That the threads list nothing
/// more is what shows they stopped.
const PROMPT: Duration = Duration::from_secs(10);

/// 2,000 files in 40 directories, 500 batches.
fn large_tree(root: &Path) -> usize {
    for dir in 0..40 {
        let dir = root.join(format!("d{dir:02}"));
        fs::create_dir(&dir).unwrap();
        for file in 0..50 {
            fs::write(dir.join(format!("f{file:02}")), "").unwrap();
        }
    }
    2_000
}

/// The most files a walk lists while its sink holds one batch: two queued,
/// one blocked per thread, and each thread's batch being filled.
fn stalled_bound() -> usize {
    let threads = walk_threads();
    BATCH * (1 + QUEUED + threads) + (BATCH - 1) * threads
}

/// What the sink does once it's let go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    GoOn,
    Panic,
}

/// What [`stall`] saw of the walk, once the sink went on.
struct Ended {
    /// Whether it ran to the end, and how many paths the sink got.
    result: thread::Result<(bool, usize)>,
    /// How long it took to end.
    took: Duration,
}

/// Walks `root` on another thread with a sink that stalls on its first
/// batch. Runs `stalled` while it's stalled, then lets the sink go on (or
/// panic) and waits for the walk to end.
fn stall<T>(
    root: &Path,
    cancel: &AtomicBool,
    walked: &Walked,
    then: Then,
    stalled: impl FnOnce() -> T,
) -> (T, Ended) {
    let (stalled_tx, stalled_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    thread::scope(|scope| {
        let walk = scope.spawn(move || {
            let mut stalled_tx = Some(stalled_tx);
            let mut got = 0;
            let done = walk_root(root, cancel, walked, |batch| {
                got += batch.len();
                if let Some(stalled) = stalled_tx.take() {
                    stalled.send(()).unwrap();
                    // Until `go_tx` is dropped.
                    let _ = go_rx.recv();
                    assert!(then == Then::GoOn, "sink");
                }
            });
            (done, got)
        });
        stalled_rx.recv().expect("the sink got a batch");
        let seen = stalled();
        let went_on = Instant::now();
        drop(go_tx);
        let result = walk.join();
        let took = went_on.elapsed();
        (seen, Ended { result, took })
    })
}

/// `listed` once it has held still for a while.
fn settled(listed: &AtomicUsize) -> usize {
    let mut last = listed.load(Ordering::Relaxed);
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        let now = listed.load(Ordering::Relaxed);
        if now == last {
            break;
        }
        last = now;
    }
    last
}

#[test]
fn a_stalled_sink_stops_the_walkers() {
    let (_dir, root) = temp();
    let total = large_tree(&root);
    let walked = Walked::with_batch(BATCH);
    let (listed, ended) = stall(&root, &AtomicBool::new(false), &walked, Then::GoOn, || {
        settled(&walked.listed);
        // Still within the bound a while later: the threads stopped, rather
        // than being slow to get going.
        thread::sleep(Duration::from_millis(300));
        walked.listed.load(Ordering::Relaxed)
    });
    // Unbounded, the threads would list every file meanwhile.
    let bound = stalled_bound();
    assert!(
        listed <= bound,
        "{listed} of {total} files listed while stalled, at most {bound}"
    );
    assert!(bound < total);
    let (done, got) = ended.result.unwrap();
    assert!(done);
    assert_eq!(got, total, "every file once the sink goes on");
}

#[test]
fn a_cancel_during_a_stall_ends_the_walk() {
    let (_dir, root) = temp();
    large_tree(&root);
    let walked = Walked::with_batch(BATCH);
    let cancel = AtomicBool::new(false);
    let (listed, ended) = stall(&root, &cancel, &walked, Then::GoOn, || {
        settled(&walked.listed);
        cancel.store(true, Ordering::Relaxed);
        walked.listed.load(Ordering::Relaxed)
    });
    assert!(listed <= stalled_bound(), "{listed} listed at the cancel");
    // Returned, so every thread is joined.
    let (done, got) = ended.result.unwrap();
    assert!(!done);
    assert!(
        ended.took < PROMPT,
        "ended {:?} after the sink went on",
        ended.took
    );
    assert_eq!(got, BATCH, "the sink isn't called again once cancelled");
    // A thread let go by the batch taken after the cancel stops at its next
    // entry.
    let after = walked.listed.load(Ordering::Relaxed);
    assert!(
        after <= listed + walk_threads(),
        "{after} listed, {listed} at the cancel"
    );
}

#[test]
fn a_panicking_sink_drops_the_receiver_and_ends_the_walk() {
    let (_dir, root) = temp();
    let total = large_tree(&root);
    let walked = Walked::with_batch(BATCH);
    let ((), ended) = stall(&root, &AtomicBool::new(false), &walked, Then::Panic, || {
        settled(&walked.listed);
    });
    assert!(ended.result.is_err(), "the sink's panic reaches the caller");
    assert!(
        ended.took < PROMPT,
        "ended {:?} after the sink went on",
        ended.took
    );
    // Unbounded, or with threads that kept going once their sends failed,
    // every file would be listed before the panic got out. A thread
    // listing when the receiver went stops at its next entry.
    let after = walked.listed.load(Ordering::Relaxed);
    let bound = stalled_bound() + walk_threads();
    assert!(
        after <= bound && bound < total,
        "{after} of {total} listed, at most {bound}"
    );
}
