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
//!   a file in place ([`open_truncated`]).
//! - [`check_ignore_file`] checks an ignore file's patterns against the project
//!   with git's matching: unused, duplicate, shadowed and invalid lines.
//!
//! This crate must never depend on GPUI.

mod finder;
#[doc(hidden)]
pub mod fixture;
pub mod fs_ops;
mod ignore_check;
mod listing;
mod search;
mod tree;
mod watcher;

pub use finder::{FileSetDelta, FinderIndex, FinderMatch, FinderStats};
pub use fs_ops::{
    NameError, create_dir, create_file, duplicate, open_truncated, relative_path, rename, trash,
    validate_name,
};
pub use ignore_check::{
    IgnoreFinding, IgnoreFindingKind, IgnoreReport, IgnoreSeverity, MAX_SCAN_ENTRIES, MatchedPaths,
    check_ignore_file, check_ignore_file_capped,
};
pub use listing::{Entry, EntryKind, Listed, Listing, open_file_under};
pub use search::{
    FileMatches, GlobError, LineMatch, PathFilter, Replacer, SearchError, SearchOptions,
    SearchQuery, SearchSummary, is_word_char, search, search_excluding, search_filtered,
};
pub use tree::{FileTree, NodeId, Row, TreeOptions, TreeStats};
pub use watcher::{FsChange, ProjectWatcher};

#[cfg(test)]
mod tests;
