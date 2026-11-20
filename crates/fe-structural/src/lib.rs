//! Structural search: ast-grep-style patterns compiled into syntax-tree
//! patterns for a language, headless. Matching them against files comes
//! later; this crate compiles them.

mod error;
mod pattern;

pub use error::{PatternError, PatternErrorKind};
pub use pattern::{Metavar, Pattern, PatternNode, Wildcard};
