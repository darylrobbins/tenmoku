// SPDX-License-Identifier: LicenseRef-Proprietary

//! How far a plugin may go: how long a call into it runs, how much memory it holds, how many
//! events wait for it and how much the host reads for it; and the limiter that holds its store
//! to them.

use std::time::Duration;

use wasmtime::{ResourceLimiter, StoreLimits, StoreLimitsBuilder};

/// How long one call into a plugin may run before it is interrupted and the plugin unloaded.
pub const CALL_DEADLINE: Duration = Duration::from_millis(100);

/// The most linear memory a plugin's store holds, across all its memories.
pub const MEMORY_BYTES: usize = 64 << 20;

/// How many events wait for a plugin before the host drops them for a `lagged`.
pub const EVENT_QUEUE: usize = 256;

/// The most bytes the host reads for a plugin at once, its component among them.
pub const MAX_READ_BYTES: usize = 16 << 20;

/// One plugin's limits. The default is the named constants; settings override them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub call_deadline: Duration,
    pub memory_bytes: usize,
    pub event_queue: usize,
    pub max_read_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            call_deadline: CALL_DEADLINE,
            memory_bytes: MEMORY_BYTES,
            event_queue: EVENT_QUEUE,
            max_read_bytes: MAX_READ_BYTES,
        }
    }
}

/// The most core instances, tables and memories a plugin's store holds, and elements in each
/// table: well past what a component needs, short of what a hostile one could exhaust.
const MAX_INSTANCES: usize = 64;
const MAX_TABLES: usize = 64;
const MAX_MEMORIES: usize = 16;
const MAX_TABLE_ELEMENTS: usize = 1 << 18;

/// What wasmtime reports when a growth is past what the memory's type can address. It reports
/// that without asking the limiter first, so nothing was charged for it.
const PAST_THE_TYPE: &str = "memory growth exceeds memory type's limits";

/// A plugin store's [`ResourceLimiter`]: [`StoreLimits`] for each memory, tables and
/// instances, with memory also capped across all the store's memories. It notes each refusal,
/// so a trap that follows one reads as running out of memory.
pub struct StoreLimiter {
    limits: StoreLimits,
    memory_bytes: usize,
    memory_used: usize,
    /// What the last growth it allowed charged, until wasmtime says it failed.
    charged: Option<usize>,
    refused: bool,
}

impl StoreLimiter {
    pub fn new(limits: &Limits) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .table_elements(MAX_TABLE_ELEMENTS)
                .instances(MAX_INSTANCES)
                .tables(MAX_TABLES)
                .memories(MAX_MEMORIES)
                .build(),
            memory_bytes: limits.memory_bytes,
            memory_used: 0,
            charged: None,
            refused: false,
        }
    }

    /// Whether a growth was refused or failed since [`Self::clear_refused`].
    pub fn refused(&self) -> bool {
        self.refused
    }

    pub fn clear_refused(&mut self) {
        self.refused = false;
    }
}

impl ResourceLimiter for StoreLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Whatever it charged before has been allocated: a failure would have come first.
        self.charged = None;
        let used = self
            .memory_used
            .saturating_sub(current)
            .saturating_add(desired);
        let allowed =
            used <= self.memory_bytes && self.limits.memory_growing(current, desired, maximum)?;
        if allowed {
            self.charged = Some(used.saturating_sub(self.memory_used));
            self.memory_used = used;
        } else {
            self.refused = true;
        }
        Ok(allowed)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        log::debug!("a plugin's memory failed to grow: {error:#}");
        if error.to_string() != PAST_THE_TYPE
            && let Some(charged) = self.charged.take()
        {
            self.memory_used -= charged;
        }
        self.refused = true;
        Ok(())
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.limits.table_growing(current, desired, maximum)?;
        self.refused |= !allowed;
        Ok(allowed)
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        log::debug!("a plugin's table failed to grow: {error:#}");
        self.refused = true;
        Ok(())
    }

    fn instances(&self) -> usize {
        self.limits.instances()
    }

    fn tables(&self) -> usize {
        self.limits.tables()
    }

    fn memories(&self) -> usize {
        self.limits.memories()
    }
}
