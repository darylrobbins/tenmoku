; Indent query for TypeScript and TSX, read by `indent.rs` after
; `javascript.scm`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/typescript/indents.scm, license Apache-2.0.
; Changes: the source's `; inherits: ecma` is done by `indent.rs`, which
; also indents between brackets itself, from the opener's row, so
; `enum_declaration` and `interface_declaration`, whose bodies are
; bracketed, no longer begin at their first row: that put a `{` on a row of
; its own a level in.

(object_type) @indent.begin
