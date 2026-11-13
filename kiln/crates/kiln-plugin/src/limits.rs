// SPDX-License-Identifier: Apache-2.0

//! How far a plugin may go: how long a call into it runs, how much memory it holds, how many
//! events wait for it and how much the host reads for it; and the limiter that holds its store
//! to them. The call deadline bounds a call's wasm and its other imports; the programs a call
//! runs (`process.spawn`) have their own [`PROGRAM_DEADLINE`], which the call deadline doesn't
//! count.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use wasmtime::{ResourceLimiter, StoreLimits, StoreLimitsBuilder};

/// How long one call into a plugin may run before it is interrupted and the plugin unloaded.
/// Time spent waiting on a program the call runs, or on a `net.fetch`, isn't counted: both
/// count against [`PROGRAM_DEADLINE`] instead.
pub const CALL_DEADLINE: Duration = Duration::from_millis(100);

/// How long a plugin's program runs and `net.fetch`es may take, all of one call together (each
/// run is killed, and each fetch's timeout capped, at what is left of the call's share). The call's
/// [`CALL_DEADLINE`] doesn't count that time, so a call holds its thread at most
/// `CALL_DEADLINE + PROGRAM_DEADLINE`.
pub const PROGRAM_DEADLINE: Duration = Duration::from_secs(10);

/// The most linear memory a plugin's store holds, across all its memories.
pub const MEMORY_BYTES: usize = 64 << 20;

/// How many events wait for a plugin before the host drops them for a `lagged`.
pub const EVENT_QUEUE: usize = 256;

/// The most bytes the host reads for a plugin at once, its component among them.
pub const MAX_READ_BYTES: usize = 16 << 20;

/// One plugin's interactive limits: what the user waits on (§3.1 of the stage 3 design). The
/// default is the named constants; settings override them. The background tier's are
/// [`BackgroundLimits`].
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

/// How long a background job runs before it is cancelled, unless its manifest declares another.
pub const BACKGROUND_WALL: Duration = Duration::from_secs(10 * 60);

/// The longest wall time a job may have, whatever is declared. No fuel metering backs it: the
/// engine has none, so wall time is what bounds a job's CPU.
pub const BACKGROUND_WALL_CEILING: Duration = Duration::from_secs(60 * 60);

/// The shortest wall time a manifest may declare.
pub const BACKGROUND_WALL_FLOOR: Duration = Duration::from_secs(1);

/// The memory a job's worker store may hold, unless its manifest declares another.
pub const BACKGROUND_MEMORY_BYTES: u64 = 256 << 20;

/// The least memory a manifest may declare for a job.
pub const BACKGROUND_MEMORY_FLOOR: u64 = 1 << 20;

/// The most one job may hold, however much RAM there is.
pub const JOB_MEMORY_CAP: u64 = 2 << 30;

/// The most all running jobs may hold together, however much RAM there is.
pub const ALL_JOBS_MEMORY_CAP: u64 = 4 << 30;

/// The most a plugin's data quota may be declared as: reserved for the data store (B7).
pub const DATA_QUOTA_CEILING: u64 = 2 << 30;

/// One plugin's background limits: what a job may use. A manifest's
/// `[contributes.plugin.background]` declares them, the host's [`Ceilings`] clamp them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundLimits {
    /// Wall time per job.
    pub wall: Duration,
    /// Memory per job, in bytes.
    pub memory_bytes: u64,
    /// The plugin data quota, in bytes. Reserved: parsed and kept, nothing enforces it yet.
    pub data_bytes: Option<u64>,
}

impl Default for BackgroundLimits {
    fn default() -> Self {
        Self {
            wall: BACKGROUND_WALL,
            memory_bytes: BACKGROUND_MEMORY_BYTES,
            data_bytes: None,
        }
    }
}

impl BackgroundLimits {
    /// Whether `self` asks for more than `granted` on `wall` or `memory_bytes`: the limits an
    /// update must ask the user about again. Lowering one, or the reserved data quota, never
    /// counts.
    pub fn raises(&self, granted: &Self) -> Vec<BackgroundLimit> {
        let mut raised = Vec::new();
        if self.memory_bytes > granted.memory_bytes {
            raised.push(BackgroundLimit::Memory);
        }
        if self.wall > granted.wall {
            raised.push(BackgroundLimit::Wall);
        }
        raised
    }
}

/// A background limit an escalation can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundLimit {
    Memory,
    Wall,
}

/// A limit declared past what the host allows, and what it was clamped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clamped {
    pub limit: BackgroundLimit,
    pub declared: u64,
    pub ceiling: u64,
}

/// What this computer allows background jobs, from its physical RAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceilings {
    pub wall: Duration,
    /// Per job: min(2 GiB, RAM / 4).
    pub job_memory_bytes: u64,
    /// All running jobs together: min(4 GiB, RAM / 2).
    pub all_jobs_memory_bytes: u64,
}

/// What [`Ceilings::host`] assumes when the platform won't say how much RAM there is.
const UNKNOWN_RAM_BYTES: u64 = 4 << 30;

impl Ceilings {
    /// The ceilings of a computer with `ram_bytes` of physical RAM.
    pub fn for_ram(ram_bytes: u64) -> Self {
        Self {
            wall: BACKGROUND_WALL_CEILING,
            job_memory_bytes: JOB_MEMORY_CAP.min(ram_bytes / 4),
            all_jobs_memory_bytes: ALL_JOBS_MEMORY_CAP.min(ram_bytes / 2),
        }
    }

    /// The ceilings of this computer.
    pub fn host() -> Self {
        Self::for_ram(kiln_core::platform::physical_ram_bytes().unwrap_or(UNKNOWN_RAM_BYTES))
    }

    /// `limits` held to these ceilings, with what was clamped. Not an error: the job runs with
    /// what the host allows.
    pub fn clamp(&self, limits: BackgroundLimits) -> (BackgroundLimits, Vec<Clamped>) {
        let mut clamped = Vec::new();
        let mut limits = limits;
        if limits.memory_bytes > self.job_memory_bytes {
            clamped.push(Clamped {
                limit: BackgroundLimit::Memory,
                declared: limits.memory_bytes,
                ceiling: self.job_memory_bytes,
            });
            limits.memory_bytes = self.job_memory_bytes;
        }
        if limits.wall > self.wall {
            clamped.push(Clamped {
                limit: BackgroundLimit::Wall,
                declared: limits.wall.as_secs(),
                ceiling: self.wall.as_secs(),
            });
            limits.wall = self.wall;
        }
        (limits, clamped)
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
