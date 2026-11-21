//! Structural search: ast-grep-style patterns compiled into syntax-tree
//! patterns for a language, matched against files' trees, and rewritten
//! with templates, headless; and a project scanned for a pattern.

mod captures;
mod constraints;
mod error;
mod matcher;
mod multi;
mod pattern;
mod prefilter;
mod rewrite;
mod scan;

pub use captures::Captures;
pub use error::{PatternError, PatternErrorKind};
pub use matcher::{Match, MatchOptions, Stopped};
pub use pattern::{Metavar, Pattern, PatternNode, Wildcard};
pub use prefilter::prefilter_literals;
pub use rewrite::{
    CHECK_LIMIT, Edit, Edits, EditsError, Rewrite, RewriteError, RewriteErrorKind, apply_edits,
};
#[cfg(feature = "test-support")]
pub use scan::testing;
pub use scan::{FIRST_LINE_BYTES, FileResult, MatchRow, ScanOptions, ScanSummary, scan};
