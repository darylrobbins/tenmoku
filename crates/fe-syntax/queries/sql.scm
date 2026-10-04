; Goes after tree-sitter-sequel's highlights.scm for SQL.
; From tree-sitter-sequel 0.3.11, https://github.com/derekstride/tree-sitter-sql,
; path queries/highlights.scm, license MIT: its two number patterns, widened to the
; forms the grammar's `_integer` and `_decimal_number` accept (radix
; prefixes, `_` separators, exponents). Nothing else is added.
;
; The crate's patterns are written for Neovim's Lua patterns, so as regexes
; they never matched and every number was a string. Later patterns win over
; earlier ones on the same node.

((literal) @number
  (#match? @number "^[-+]?(0[xX][0-9A-Fa-f]+(_[0-9A-Fa-f]+)*|0[oO][0-7]+(_[0-7]+)*|0[bB][01]+(_[01]+)*|\\d+(_\\d+)*(e[+-]?\\d+(_\\d+)*)?)$"))

((literal) @float
  (#match? @float "^[-+]?((\\d+(_\\d+)*)?[.]\\d+(_\\d+)*(e[+-]?\\d+(_\\d+)*)?|\\d+(_\\d+)*[.](e[+-]?\\d+(_\\d+)*)?)$"))
