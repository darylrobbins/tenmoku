; Indent query for CSS, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/css/indents.scm, license Apache-2.0.
; Changes: the source's `"}" @indent.dedent` is left out. It takes back the
; level its block adds to the rows after it, which ours doesn't add: a block
; ending in a `}` doesn't indent the rows after its last.

[
  (block)
  (declaration)
] @indent.begin

(block
  "}" @indent.branch)

(comment) @indent.ignore
