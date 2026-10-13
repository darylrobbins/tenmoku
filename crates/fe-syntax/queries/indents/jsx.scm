; Indent query for JSX, read by `indent.rs` after `javascript.scm` for
; JavaScript and TSX.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/jsx/indents.scm, license Apache-2.0.
; Changes: none.

[
  (jsx_element)
  (jsx_self_closing_element)
  (jsx_expression)
] @indent.begin

(jsx_closing_element
  ">" @indent.end)

(jsx_self_closing_element
  "/>" @indent.end)

[
  (jsx_closing_element)
  ">"
] @indent.branch

; <button
; />
(jsx_self_closing_element
  "/>" @indent.branch)
