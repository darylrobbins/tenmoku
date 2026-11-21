//! [`WatchHost`]: changes on a host's disk, as fe-project's `ProjectWatcher`
//! reports them (D § 5).

use std::{
    io,
    path::{Path, PathBuf},
};

use crate::Subscription;

/// Something that may have changed on disk: what a watch delivers, in
/// batches, and the wire type unchanged (D § 5). The type was fe-project's
/// (`watcher.rs`), which re-exports it where it was.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FsChange {
    /// Something at this path may have been created, changed or removed.
    Path(PathBuf),
    /// Events were dropped or coalesced; anything may have changed.
    Rescan,
}

/// A host's file watching.
pub trait WatchHost: Send + Sync {
    /// Watches `root` recursively, delivering batches of [`FsChange`]s
    /// until the subscription is dropped, which stops the watch. Its local
    /// impl is fe-project's `LocalWatch`, a `ProjectWatcher`: a batch is
    /// every event queued when its thread wakes, with each path once, and
    /// its docs have when it sends [`FsChange::Rescan`]. A watch that
    /// can't start fails here.
    fn watch(&self, root: &Path) -> io::Result<Subscription<Vec<FsChange>>>;
}
