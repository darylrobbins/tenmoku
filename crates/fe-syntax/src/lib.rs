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
//!
//! This crate must never depend on GPUI.

mod driver;
mod highlight;
mod injection;
mod language;
mod syntax;

pub use driver::{Spawner, SyntaxDriver};
pub use highlight::{Highlight, HighlightSpan, resolve_theme};
pub use language::{
    Language, language_by_name, language_for_path, language_name_for_path, language_names,
    loaded_language,
};
pub use syntax::{MAX_PARSE_BYTES, ParseJob, Parsed, Syntax};

#[cfg(test)]
mod tests;
