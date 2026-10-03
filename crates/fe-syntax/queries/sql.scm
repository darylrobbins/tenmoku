; Goes after tree-sitter-sequel's highlights.scm for SQL.
; From tree-sitter-sequel 0.3.11, https://github.com/derekstride/tree-sitter-sql,
; path queries/highlights.scm, license MIT: its two number patterns, restated
; with `\d` for the Lua class `%d`. Nothing else is added.
;
; The crate's patterns are written for Neovim's Lua patterns, so as regexes
; they never matched and every number was a string. Later patterns win over
; earlier ones on the same node.

((literal) @number
  (#match? @number "^[-+]?\\d+$"))

((literal) @float
  (#match? @float "^[-+]?\\d*\\.\\d*$"))
