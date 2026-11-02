//! The JSON5 grammar for tree-sitter, in the shape the crates.io grammars
//! expose: a `LANGUAGE` and a `HIGHLIGHTS_QUERY`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_json5() -> *const ();
}

/// The tree-sitter language for JSON5 files.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_json5) };

/// The content of `node-types.json`.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The highlight query, as upstream ships it.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
