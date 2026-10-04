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
//!
//! This crate must never depend on GPUI.

mod detect;
mod driver;
mod fold;
mod highlight;
mod injection;
mod language;
mod syntax;

pub use detect::{detect_language_name, language_name_for_shebang};
pub use driver::{Spawner, SyntaxDriver};
pub use fold::{fold_range_at, fold_ranges};
pub use highlight::{Highlight, HighlightSpan, resolve_theme};
pub use language::{
    Language, language_by_name, language_for_path, language_name_for_path, language_names,
    loaded_language,
};
pub use syntax::{MAX_PARSE_BYTES, ParseJob, Parsed, Syntax};

#[cfg(test)]
mod tests;
