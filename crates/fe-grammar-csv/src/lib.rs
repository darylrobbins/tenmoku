//! CSV and TSV for tree-sitter, in which a column's index is its position among
//! its row's children. See `grammar.js`.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_csv() -> *const ();
    fn tree_sitter_tsv() -> *const ();
}

/// The tree-sitter language for comma-separated files.
pub const LANGUAGE_CSV: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_csv) };

/// The tree-sitter language for tab-separated files.
pub const LANGUAGE_TSV: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_tsv) };
