; Indent query for Rust, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/rust/indents.scm, license Apache-2.0.
; Changes: the source's repeated patterns (`trait_item`, `string_literal` and
; the `macro_rules!` ERROR) appear once. `indent.rs` indents between brackets
; itself, from the opener's row, so the items whose bodies are bracketed
; (`mod_item`, `struct_item`, `enum_item`, `impl_item`, `struct_expression`
; and `trait_item`'s body) no longer begin at their first row, which put a
; `{` on a row of its own a level in, and `impl_item`'s body is no longer a
; branch. A `where` clause begins without the source's dedent and `where`
; branch, which assumed `impl_item` begins. `binary_expression` begins, and
; the `else_clause` pattern at the end is ours.

[
  (struct_pattern)
  (tuple_struct_pattern)
  (tuple_expression)
  (tuple_type)
  (tuple_pattern)
  (match_block)
  (call_expression)
  (assignment_expression)
  (arguments)
  (block)
  (where_clause)
  (use_list)
  (array_expression)
  (ordered_field_declaration_list)
  (field_declaration_list)
  (enum_variant_list)
  (parameters)
  (token_tree)
  (token_repetition)
  (macro_definition)
  (binary_expression)
] @indent.begin

(string_literal
  (escape_sequence)) @indent.begin

; Typing in "(" inside macro definitions breaks the tree entirely
; Making macro_definition becoming errors
; Offset this by adding back one indent for start of macro rules
(ERROR
  .
  "macro_rules!"
  [
    "("
    "{"
    "["
  ] @indent.begin
  (#set! indent.immediate)
  (#set! indent.start_at_same_line))

(macro_definition
  [
    ")"
    "}"
    "]"
  ] @indent.end)

(block
  "}" @indent.end)

(enum_item
  body: (enum_variant_list
    "}" @indent.end))

(impl_item
  body: (declaration_list
    "}" @indent.end))

(match_expression
  body: (match_block
    "}" @indent.end))

(mod_item
  body: (declaration_list
    "}" @indent.end))

(struct_item
  body: (field_declaration_list
    "}" @indent.end))

(struct_expression
  body: (field_initializer_list
    "}" @indent.end))

(struct_pattern
  "}" @indent.end)

(tuple_struct_pattern
  ")" @indent.end)

(tuple_type
  ")" @indent.end)

(tuple_pattern
  ")" @indent.end)

(trait_item
  body: (declaration_list
    "}" @indent.end))

[
  ")"
  "]"
  "}"
] @indent.branch

[
  (line_comment)
  (string_literal)
] @indent.ignore

(raw_string_literal) @indent.auto

; Ours: an `else` starting a row is a branch, aligned with its `if`. It also
; begins, on its own row only, so the two cancel and its row keeps the
; `if`'s level, which no capture raises.
(else_clause
  "else" @indent.branch @indent.begin
  (#set! indent.start_at_same_line))
