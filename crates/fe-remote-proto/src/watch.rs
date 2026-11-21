//! Channel 2's bodies: `WatchHost` (§ 5).

use crate::PathBytes;
use serde::{Deserialize, Serialize};

/// A `Request` body on channel 2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchOp {
    /// Watches `root` recursively. The request stays open: each `Data`
    /// frame is a batch of changes, a `Vec<FsChange>`, until `Cancel`.
    Watch { root: PathBytes },
}

/// fe-project's `FsChange`: something that may have changed on the host.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FsChange {
    /// Something at this path may have been created, changed or removed.
    Path(PathBytes),
    /// Events were dropped or coalesced; anything may have changed.
    Rescan,
}
