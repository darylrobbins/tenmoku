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
//! - [`SymbolIndex::plan`] keeps it current from watcher batches, naming
//!   the files to extract again ([`extract_all`]) for
//!   [`SymbolIndex::update`].
//! - [`NavigationProvider`] is the seam navigation answers come through:
//!   by search here, precisely from a language server elsewhere.
//!   [`Navigation`] routes each request to the best provider ready for its
//!   language.
//! - [`SearchNavigation`] answers go-to-definition and workspace symbols
//!   from a [`SymbolIndex`], with a capped text search while it's
//!   incomplete, and finds references by a word search of the project,
//!   dropping the hits a parse puts in comments and strings.
//!
//! Like [`fe_project::search`], nothing here owns an executor: the caller
//! runs [`build`] on a background thread of its own, and the workers are
//! scoped to that call.
//!
//! This crate must never depend on GPUI.

mod build;
mod extract;
mod index;
mod provider;
mod references;
mod search;

pub use build::{
    BuildOptions, BuildSummary, Extracted, MAX_BATCH_FILES, SkipCounts, build, extract_all,
};
pub use extract::{FileSymbols, Skipped, Symbol, extract};
pub use fe_syntax::OutlineKind;
pub use index::{IndexStats, Reindex, SymbolIndex, SymbolMatch, SymbolRef};
pub use provider::{
    Answer, Definition, Definitions, FileReferences, Location, NavError, NavRequest, Navigation,
    NavigationProvider, Precision, Readiness, Reference, ReferencesSummary, Tier, WorkspaceSymbol,
};
pub use references::ReferenceOptions;
pub use search::{SearchNavigation, SearchStats};
