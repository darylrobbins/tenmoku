//! Structural search: ast-grep-style patterns compiled into syntax-tree
//! patterns for a language, and matched against files' trees, headless.

mod captures;
mod error;
mod matcher;
mod pattern;

pub use captures::Captures;
pub use error::{PatternError, PatternErrorKind};
pub use matcher::{Match, MatchOptions, Stopped};
pub use pattern::{Metavar, Pattern, PatternNode, Wildcard};
