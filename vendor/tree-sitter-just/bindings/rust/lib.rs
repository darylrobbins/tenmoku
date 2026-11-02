//! The just grammar for tree-sitter, in the shape the crates.io grammars
//! expose: a `LANGUAGE`, a `HIGHLIGHTS_QUERY` and an `INJECTIONS_QUERY`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_just() -> *const ();
}

/// The tree-sitter language for justfiles.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_just) };

/// The content of `node-types.json`.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// Upstream's highlight query.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries-src/highlights.scm");

/// Upstream's injection query: recipe bodies in bash or their shebang's language.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries-src/injections.scm");
