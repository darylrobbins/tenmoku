; Used in place of tree-sitter-rust's injections.scm for Rust.
; From tree-sitter-rust 0.24.2, https://github.com/tree-sitter/tree-sitter-rust,
; path queries/injections.scm, license MIT. Changes: doc comments are
; injected as inline Markdown, and the two token-tree patterns are dropped.
; Injecting macro bodies as Rust re-highlights them (`String::from` becomes a
; constructor, calls gain method captures), which changes the golden sample;
; the outer parse already highlights them as Rust.
;
; A `doc_comment` is the text of a `///`, `//!` or `/** */` comment after its
; marker. Combining them parses a run of comments as one inline-Markdown
; layer, so a code span may wrap lines.

((doc_comment) @injection.content
 (#set! injection.language "markdown_inline")
 (#set! injection.combined))
