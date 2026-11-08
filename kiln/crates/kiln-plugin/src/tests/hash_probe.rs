// SPDX-License-Identifier: Apache-2.0

//! Under test, slows a deadlined check's reads and counts the chunks it read, per thread.

use std::cell::Cell;
use std::time::Duration;

thread_local! {
    static DELAY: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    static CHUNKS: Cell<usize> = const { Cell::new(0) };
}

/// Each chunk read from here on takes `delay` more; none counted yet.
pub(crate) fn slow_reads(delay: Duration) {
    DELAY.set(delay);
    CHUNKS.set(0);
}

/// The chunks read since [`slow_reads`].
pub(crate) fn chunks() -> usize {
    CHUNKS.get()
}

pub(crate) fn after_chunk() {
    CHUNKS.set(CHUNKS.get() + 1);
    std::thread::sleep(DELAY.get());
}
