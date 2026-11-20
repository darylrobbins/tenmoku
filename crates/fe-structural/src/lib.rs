//! Structural search: ast-grep-style patterns compiled into syntax-tree
//! patterns for a language, matched against files' trees, and rewritten
//! with templates, headless.

mod captures;
mod constraints;
mod error;
mod matcher;
mod multi;
mod pattern;
mod rewrite;

pub use captures::Captures;
pub use error::{PatternError, PatternErrorKind};
pub use matcher::{Match, MatchOptions, Stopped};
pub use pattern::{Metavar, Pattern, PatternNode, Wildcard};
pub use rewrite::{
    CHECK_LIMIT, Edit, Edits, EditsError, Rewrite, RewriteError, RewriteErrorKind, apply_edits,
};
