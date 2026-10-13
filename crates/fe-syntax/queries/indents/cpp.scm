; Indent query for C++, read by `indent.rs` after C's (`c.scm`).
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/cpp/indents.scm, license Apache-2.0.
; Changes: the source's `; inherits: c` is done by `indent.rs`. The
; range-based `for` pattern at the end is ours.

(condition_clause) @indent.begin

((field_initializer_list) @indent.begin
  (#set! indent.start_at_same_line 1))

(access_specifier) @indent.branch

; Ours: a range-based `for` with an unbraced body begins, as C's loops do.
((for_range_loop
  body: (_) @_body) @indent.begin
  (#not-kind-eq? @_body "compound_statement"))
