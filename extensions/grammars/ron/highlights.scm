; From tree-sitter-grammars/tree-sitter-ron queries/highlights.scm at
; 78938553b93075e638035f624973083451b29055, licence MIT OR Apache-2.0 (see
; LICENSE and LICENSE-APACHE). Dropped: @spell (no colour) and the (ERROR) @error
; pattern. nvim-treesitter style, so later patterns win.

; Structs
;------------

(enum_variant) @constant
(struct_entry (identifier) @property)
(struct_entry (enum_variant (identifier) @constant))
(struct_name (identifier)) @type

(unit_struct) @type.builtin


; Literals
;------------

(string) @string
(boolean) @boolean
(integer) @number
(float) @float
(char) @character


; Comments
;------------

[
  (line_comment)
  (block_comment)
] @comment


; Punctuation
;------------

["{" "}"] @punctuation.bracket

["(" ")"] @punctuation.bracket

["[" "]"] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter

[
  "-"
] @operator

; Special
;------------

(escape_sequence) @string.escape
