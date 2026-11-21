//! The project model for fastedit.
//!
//! - [`FileTree`] is a lazy file tree: a directory is listed only when it is
//!   expanded, entries ignored by `.gitignore`/`.ignore` are hidden, and a
//!   cached flattened view serves a virtual list a range of rows at a time.
//!   Listing can run off the UI thread ([`FileTree::listing`], [`Listing::run`],
//!   [`FileTree::fill`]).
//! - [`ProjectWatcher`] watches the project and delivers batches of
//!   [`FsChange`]s, which [`FileTree::apply`] reconciles with the disk.
//! - [`FinderIndex`] holds every non-ignored file path, walked up front on a
//!   background thread ([`FinderIndex::walk`]), for fuzzy matching with
//!   [`FinderIndex::query`] (Cmd-P). [`FinderIndex::apply`] keeps it current
//!   from the same batches.
//! - [`search()`] searches the contents of the files the tree shows, with
//!   ripgrep's searcher, streaming capped and cancellable results;
//!   [`search_excluding`] leaves one file out.
//! - [`open_file_under`] opens a path the tree or the finder listed without
//!   following a symlink, for readers of the project's files.
//! - [`fs_ops`] creates, renames, duplicates and trashes entries of the tree,
//!   all kept under the project root ([`create_file`], [`create_dir`],
//!   [`rename`], [`duplicate`], [`trash`], [`relative_path`]), and empties
//!   a file in place ([`open_truncated`]). [`LocalTreeFs`] is fe-host's
//!   `FsHost` with them, and [`Listing::run_on`] lists through one.
//! - [`Markers`] are the build files in a folder, for module and source-root
//!   icons: found by a [`MarkerProbe`], one read of the folder's names
//!   through a no-follow handle and at most [`MAX_STATS`] metadata calls,
//!   cached by [`MarkerCache`] until a watcher batch names the folder.
//! - [`check_ignore_file`] checks an ignore file's patterns against the project
//!   with git's matching: unused, duplicate, shadowed and invalid lines.
//!
//! This crate must never depend on GPUI.

#[cfg(windows)]
pub mod beneath;
mod docker_ignore;
mod finder;
#[doc(hidden)]
pub mod fixture;
mod folder_markers;
pub mod fs_ops;
mod ignore_check;
pub mod l10n;
mod listing;
mod search;
mod tree;
mod watcher;
#[cfg(windows)]
mod win;

pub use finder::{FileSetDelta, FinderIndex, FinderMatch, FinderStats};
pub use folder_markers::{MAX_READ, MAX_STATS, MarkerCache, MarkerProbe, Markers, Probed};
pub use fs_ops::{
    LocalTreeFs, NameError, create_dir, create_file, duplicate, local_hosts, open_truncated,
    relative_path, rename, trash, validate_name,
};
pub use ignore_check::{
    IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, MAX_SCAN_ENTRIES, MatchedPaths,
    check_ignore_file, check_ignore_file_capped,
};
pub use listing::{Entry, EntryKind, Listed, Listing, open_file_under};
pub use search::{
    FileMatches, GlobError, LineMatch, PathFilter, Replacer, SearchError, SearchOptions,
    SearchQuery, SearchSummary, WalkControl, WalkSummary, WalkedFile, is_word_char, search,
    search_excluding, search_filtered, walk_files,
};
pub use tree::{FileTree, NodeId, Row, TreeOptions, TreeStats};
pub use watcher::{FsChange, ProjectWatcher};

#[cfg(test)]
mod tests;
