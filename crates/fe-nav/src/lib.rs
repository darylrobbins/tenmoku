//! Search-based navigation for fastedit: an index of every definition in
//! the project, by name, for instant go-to-def and workspace symbols before
//! (or without) a language server.
//!
//! - [`extract`] reads one file as the editor's loader does and lists its
//!   definitions with [`fe_syntax::symbols`], as [`FileSymbols`].
//! - [`SymbolIndex`] holds them compactly, finds a name's definitions by a
//!   hash lookup ([`SymbolIndex::definitions`]) and fuzzy-matches names
//!   ([`SymbolIndex::fuzzy`]).
//! - [`build`] walks the project as its tree shows it
//!   ([`fe_project::FinderIndex::walk`]) and extracts files on worker
//!   threads, handing batches to a sink that extends an index.
//!
//! Like [`fe_project::search`], nothing here owns an executor: the caller
//! runs [`build`] on a background thread of its own, and the workers are
//! scoped to that call.
//!
//! This crate must never depend on GPUI.

mod build;
mod extract;
mod index;

pub use build::{BuildOptions, BuildSummary, MAX_BATCH_FILES, SkipCounts, build};
pub use extract::{FileSymbols, Skipped, Symbol, extract};
pub use fe_syntax::OutlineKind;
pub use index::{IndexStats, SymbolIndex, SymbolMatch, SymbolRef};
