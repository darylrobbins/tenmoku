//! The gitignore grammar for tree-sitter, in the shape the crates.io grammars
//! expose: a `LANGUAGE` and a `HIGHLIGHTS_QUERY`.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_gitignore() -> *const ();
}

/// The tree-sitter language for gitignore files.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_gitignore) };

/// The content of `node-types.json`.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The highlight query. Upstream ships none; this one is ours.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading gitignore parser");
    }
}
