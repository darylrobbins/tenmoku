; Indent query for Java, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/java/indents.scm, license Apache-2.0.
; Changes: `indent.rs` indents between brackets itself, from the opener's
; row, so `(`, `{` and `[` are no longer branches: that put one on a row of
; its own out of line. The loop, `do` and `if` patterns at the end, for
; unbraced bodies and `else`, and the case pattern, are ours.

[
  (class_body)
  (enum_body)
  (interface_body)
  (constructor_body)
  (annotation_type_body)
  (block)
  (switch_block)
  (array_initializer)
  (argument_list)
  (formal_parameters)
  (annotation_argument_list)
  (element_value_array_initializer)
] @indent.begin

(expression_statement
  (method_invocation) @indent.begin)

[
  ")"
  "}"
  "]"
] @indent.branch

(annotation_argument_list
  ")" @indent.end)

"}" @indent.end

; Ours: a case's statements are a level in from its label, and so is a new
; row after the label.
((switch_block_statement_group) @indent.begin
  (#set! indent.immediate))

(line_comment) @indent.ignore

[
  (ERROR)
  (block_comment)
] @indent.auto

; Ours: a loop with an unbraced body begins, as C's do.
([
  (for_statement
    body: (_) @_body)
  (enhanced_for_statement
    body: (_) @_body)
  (while_statement
    body: (_) @_body)
] @indent.begin
  (#not-kind-eq? @_body "block"))

; Ours: an unbraced `do` body is a level in from the `do`, and so is a new
; row after the `do` it follows; the `while` lines up with the `do`.
(do_statement
  body: (_) @indent.begin
  (#not-kind-eq? @indent.begin "block")
  (#set! indent.start_at_same_line))

(do_statement
  "do" @indent.begin
  body: (_) @_body
  (#not-kind-eq? @_body "block")
  (#set! indent.immediate))

; Ours: an unbraced body, on the `if`'s row or its own, is a level in, and
; so is a new row after the condition or `else` it follows.
(if_statement
  consequence: (_) @indent.begin
  (#not-kind-eq? @indent.begin "block")
  (#set! indent.start_at_same_line))

(if_statement
  alternative: (_) @indent.begin
  (#not-kind-eq? @indent.begin "block" "if_statement")
  (#set! indent.start_at_same_line))

(if_statement
  condition: (parenthesized_expression
    ")" @indent.begin)
  consequence: (_) @_consequence
  (#not-kind-eq? @_consequence "block")
  (#set! indent.immediate))

(if_statement
  "else" @indent.begin
  alternative: (_) @_alternative
  (#not-kind-eq? @_alternative "block" "if_statement")
  (#set! indent.immediate))

; Ours: an `else` starting a row is a branch, aligned with its `if`. It also
; begins, on its own row only, so the two cancel and its row keeps the
; `if`'s level.
(if_statement
  "else" @indent.branch @indent.begin
  (#set! indent.start_at_same_line))
