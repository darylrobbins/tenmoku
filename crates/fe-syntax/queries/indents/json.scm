; Indent query for JSON, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/json/indents.scm, license Apache-2.0.
; Changes: none.

[
  (object)
  (array)
] @indent.begin

[
  "}"
  "]"
] @indent.branch

(comment) @indent.ignore
