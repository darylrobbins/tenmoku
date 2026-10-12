//! Syntax trees and highlighting for fastedit, on tree-sitter.
//!
//! - [`Language`]s come from grammar crates, one Cargo feature each, and are
//!   chosen by file name with [`language_for_path`]. Each uses the highlight
//!   and injections queries its grammar ships. Injected languages, such as
//!   Markdown's inline markup and fenced code, are highlighted too.
//! - [`Syntax`] keeps a buffer's tree up to date incrementally from
//!   [`fe_buffer::BufferSnapshot`]s. Parsing can run on another thread via
//!   [`ParseJob`], so a UI never has to wait for it. [`SyntaxDriver`] decides
//!   where each reparse runs, starting background ones through a [`Spawner`].
//! - Grammar capture names map onto our own [`Highlight`] vocabulary, which
//!   themes colour (see [`resolve_theme`]).
//! - [`fold_ranges`] and [`fold_range_at`] find the rows a tree's blocks,
//!   bodies, lists and comments span, for folding.
//! - [`outline`] lists a parsed file's symbols, nested, from our own
//!   per-language outline queries; [`outline_job`] runs it on another thread.
//!   [`symbols`] adds fields, enum variants and variables, for a tags index.
//!
//! This crate must never depend on GPUI.

mod comment;
mod detect;
mod driver;
mod fold;
mod highlight;
mod imports;
mod injection;
mod language;
mod outline;
mod parse_limit;
mod syntax;

pub use comment::{CommentTokens, comment_tokens};
pub use detect::{detect_language_name, language_name_for_shebang};
pub use driver::{Spawner, SyntaxDriver};
pub use fold::{fold_range_at, fold_ranges};
pub use highlight::{Highlight, HighlightSpan, resolve_theme};
pub use imports::{ImportsError, module_specifiers, module_specifiers_supported};
pub use language::{
    Language, language_by_name, language_for_path, language_name_for_path, language_names,
    loaded_language,
};
pub use outline::{
    MAX_OUTLINE_ITEMS, NoOutline, Outline, OutlineItem, OutlineJob, OutlineKind, outline,
    outline_job, outline_supported, symbols, symbols_job, symbols_supported,
};
pub use parse_limit::TREE_BUDGET_BYTES;
pub use syntax::{ParseJob, Parsed, Syntax};

#[cfg(test)]
mod tests;
