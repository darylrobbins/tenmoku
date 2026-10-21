; Used in place of tree-sitter-rust's injections.scm for Rust.
; From tree-sitter-rust 0.24.2, https://github.com/tree-sitter/tree-sitter-rust,
; path queries/injections.scm, license MIT. Changes: doc comments are
; injected as inline Markdown (`rustdoc_inline`, link text only), and the two
; token-tree patterns are dropped.
; Injecting macro bodies as Rust re-highlights them (`String::from` becomes a
; constructor, calls gain method captures), which changes the golden sample;
; the outer parse already highlights them as Rust.
;
; A `doc_comment` is the text of a `///`, `//!` or `/** */` comment after its
; marker. `injection.combined` joins nodes sharing a parent, but each
; `doc_comment` has its own `line_comment` parent, so a run of `///` lines is
; one layer per line and a link may not wrap lines. Code spans no longer
; matter for that, as they keep the comment colour.

((doc_comment) @injection.content
 (#set! injection.language "rustdoc_inline")
 (#set! injection.combined))
