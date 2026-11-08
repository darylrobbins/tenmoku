// SPDX-License-Identifier: Apache-2.0

//! Controlled delays and failures in pre-launch work. Those on the plugin's thread are per
//! thread; those on the store's writer, which runs on a thread of its own, are per store.

use std::cell::Cell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

thread_local! {
    static SETUP_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    static OPEN_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    static LAUNCHES: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Store {
    /// Writes wait until then.
    pub(crate) held_until: Option<Instant>,
    /// So many tries to write fail.
    pub(crate) failing: usize,
    /// The writer's thread can't start.
    pub(crate) no_threads: bool,
    /// The tries to write so far.
    pub(crate) tries: usize,
    /// The writes that finished, whether or not they changed anything.
    pub(crate) written: usize,
}

static STORES: Mutex<Option<HashMap<PathBuf, Store>>> = Mutex::new(None);

/// Changes, then reads, what the seam holds for the store at `path`.
pub(crate) fn store(path: &Path, change: impl FnOnce(&mut Store)) -> Store {
    let mut stores = STORES.lock().unwrap();
    let store = stores
        .get_or_insert_default()
        .entry(path.to_owned())
        .or_default();
    change(store);
    *store
}

fn wait_until(until: Option<Instant>) {
    if let Some(until) = until {
        std::thread::sleep(until.saturating_duration_since(Instant::now()));
    }
}

/// This thread's next run setup takes until `until`.
pub(crate) fn delay_setup(until: Instant) {
    SETUP_UNTIL.set(Some(until));
}

/// This thread's next opening of a program takes until `until`.
pub(crate) fn delay_open(until: Instant) {
    OPEN_UNTIL.set(Some(until));
}

pub(crate) fn during_setup() {
    wait_until(SETUP_UNTIL.take());
}

/// A child is about to start.
pub(crate) fn launching() {
    LAUNCHES.set(LAUNCHES.get() + 1);
}

/// The children this thread has started, killed at once or not.
pub(crate) fn launches() -> usize {
    LAUNCHES.get()
}

pub(crate) fn after_open() {
    wait_until(OPEN_UNTIL.take());
}

pub(crate) fn thread_start(path: &Path) -> io::Result<()> {
    if store(path, |_| {}).no_threads {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    Ok(())
}

pub(crate) fn store_io(path: &Path) -> io::Result<()> {
    while let Some(until) = store(path, |_| {}).held_until
        && Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut failing = false;
    store(path, |store| {
        store.tries += 1;
        failing = store.failing > 0;
        store.failing = store.failing.saturating_sub(1);
    });
    if failing {
        return Err(io::ErrorKind::StorageFull.into());
    }
    Ok(())
}

pub(crate) fn written(path: &Path) {
    store(path, |store| store.written += 1);
}
