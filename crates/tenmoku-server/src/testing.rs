//! Fault injection for this crate's tests (feature `test-support`).

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

static REAPERS_FAILING: AtomicUsize = AtomicUsize::new(0);

/// Makes the next `count` session-reaper threads fail to start, in every
/// daemon of this process.
pub fn fail_reaper_threads(count: usize) {
    REAPERS_FAILING.store(count, Ordering::SeqCst);
}

pub(crate) fn injected_reaper_failure() -> io::Result<()> {
    let took =
        REAPERS_FAILING.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    match took {
        Ok(_) => Err(io::Error::other("injected thread start failure")),
        Err(_) => Ok(()),
    }
}
