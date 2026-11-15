; Used in place of tree-sitter-rust's injections.scm for Rust.
; From tree-sitter-rust 0.24.2, https://github.com/tree-sitter/tree-sitter-rust,
; path queries/injections.scm, license MIT. Changes: doc comments are
; injected as inline Markdown (`rustdoc_inline`, link text only), and the two
; token-tree patterns are dropped.
; Injecting macro bodies as Rust re-highlights them (`String::from` becomes a
; constructor, calls gain method captures), which changes the golden sample;
; the outer parse already highlights them as Rust.
;
; The regex grammar is injected into the pattern strings of Regex::new,
; RegexBuilder::new, RegexSet::new (each element of the array), their
; bytes:: forms and regex! macros. Limit: raw strings (r"..", r#".."#) are
; injected whole (the content, not the quotes); an ordinary string only when
; its content has no backslash, as Rust's `\\d` is two characters in source.
;
; A `doc_comment` is the text of a `///`, `//!` or `/** */` comment after its
; marker. `injection.combined` joins nodes sharing a parent, but each
; `doc_comment` has its own `line_comment` parent, so a run of `///` lines is
; one layer per line and a link may not wrap lines. Code spans no longer
; matter for that, as they keep the comment colour.

((doc_comment) @injection.content
 (#set! injection.language "rustdoc_inline")
 (#set! injection.combined))

(call_expression
  function: (scoped_identifier
    path: [
      (identifier) @_type
      (scoped_identifier name: (identifier) @_type)
    ]
    name: (identifier) @_fn)
  arguments: (arguments
    .
    (raw_string_literal (string_content) @injection.content))
  (#any-of? @_type "Regex" "RegexBuilder")
  (#eq? @_fn "new")
  (#set! injection.language "regex"))

(call_expression
  function: (scoped_identifier
    path: [
      (identifier) @_type
      (scoped_identifier name: (identifier) @_type)
    ]
    name: (identifier) @_fn)
  arguments: (arguments
    .
    (string_literal . (string_content) @injection.content .))
  (#any-of? @_type "Regex" "RegexBuilder")
  (#eq? @_fn "new")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))

; RegexSet::new([r"a", r"b"])
(call_expression
  function: (scoped_identifier
    path: [
      (identifier) @_type
      (scoped_identifier name: (identifier) @_type)
    ]
    name: (identifier) @_fn)
  arguments: (arguments
    .
    (array_expression
      (raw_string_literal (string_content) @injection.content)))
  (#any-of? @_type "RegexSet" "RegexSetBuilder")
  (#eq? @_fn "new")
  (#set! injection.language "regex"))

(call_expression
  function: (scoped_identifier
    path: [
      (identifier) @_type
      (scoped_identifier name: (identifier) @_type)
    ]
    name: (identifier) @_fn)
  arguments: (arguments
    .
    (array_expression
      (string_literal . (string_content) @injection.content .)))
  (#any-of? @_type "RegexSet" "RegexSetBuilder")
  (#eq? @_fn "new")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))

; regex!(r"a+b")
(macro_invocation
  macro: [
    (identifier) @_macro
    (scoped_identifier name: (identifier) @_macro)
  ]
  (token_tree
    .
    (raw_string_literal (string_content) @injection.content))
  (#eq? @_macro "regex")
  (#set! injection.language "regex"))

(macro_invocation
  macro: [
    (identifier) @_macro
    (scoped_identifier name: (identifier) @_macro)
  ]
  (token_tree
    .
    (string_literal . (string_content) @injection.content .))
  (#eq? @_macro "regex")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))
