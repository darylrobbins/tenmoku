//! The git-config grammar for tree-sitter, in the shape the crates.io grammars
//! expose: a `LANGUAGE` and a `HIGHLIGHTS_QUERY`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_git_config() -> *const ();
}

/// The tree-sitter language for git-config files.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_git_config) };

/// The content of `node-types.json`.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// Upstream's highlight query.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
