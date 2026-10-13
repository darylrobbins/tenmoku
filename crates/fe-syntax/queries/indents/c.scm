; Indent query for C, read by `indent.rs`, and the first part of C++'s.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/c/indents.scm, license Apache-2.0.
; Changes: we don't support `@indent.align`, so the source's aligned argument
; and parameter lists begin instead. `indent.rs` indents between brackets
; itself, from the opener's row, so a `{` after `else` is no longer a branch:
; that put a `{` on a row of its own out of line. Cases begin immediately, as
; their bodies have no closer, and a case's `break` dedents the row after it. The
; source's unbraced `if` pattern, which begins the whole `if` and dedents
; the `else` body, is replaced by the body patterns after it, which with the
; `do`, `else` and `break` patterns are ours.

[
  (compound_statement)
  (field_declaration_list)
  (enumerator_list)
  (compound_literal_expression)
  (initializer_list)
  (init_declarator)
] @indent.begin

((case_statement) @indent.begin
  (#set! indent.immediate))

; Ours: a case's `break` ends it.
(case_statement
  (break_statement) @indent.dedent
  (#set! indent.immediate))

; With current indent logic, if we capture expression_statement with @indent.begin
; It will be affected by _parent_ node with error subnodes deep down the tree
; So narrow indent capture to check for error inside expression statement only,
(expression_statement
  (_) @indent.begin
  ";" @indent.end)

(ERROR
  "for"
  "(" @indent.begin
  ";"
  ";"
  ")" @indent.end)

((for_statement
  body: (_) @_body) @indent.begin
  (#not-kind-eq? @_body "compound_statement"))

(while_statement
  condition: (_) @indent.begin)

((while_statement
  body: (_) @_body) @indent.begin
  (#not-kind-eq? @_body "compound_statement"))

((if_statement)
  .
  (ERROR
    "else" @indent.begin))

(if_statement
  condition: (_) @indent.begin)

; Ours: an unbraced `do` body is a level in from the `do`, and so is a new
; row after the `do` it follows; the `while` lines up with the `do`.
(do_statement
  body: (_) @indent.begin
  (#not-kind-eq? @indent.begin "compound_statement")
  (#set! indent.start_at_same_line))

(do_statement
  "do" @indent.begin
  body: (_) @_body
  (#not-kind-eq? @_body "compound_statement")
  (#set! indent.immediate))

; Ours: an unbraced body, on the `if`'s row or its own, is a level in, and
; so is a new row after the condition or `else` it follows.
(if_statement
  consequence: (_) @indent.begin
  (#not-kind-eq? @indent.begin "compound_statement")
  (#set! indent.start_at_same_line))

(if_statement
  condition: (_
    ")" @indent.begin .)
  consequence: (_) @_consequence
  (#not-kind-eq? @_consequence "compound_statement")
  (#set! indent.immediate))

(else_clause
  (_) @indent.begin
  (#not-kind-eq? @indent.begin "compound_statement" "if_statement")
  (#set! indent.start_at_same_line))

(else_clause
  "else" @indent.begin
  (_) @_alternative
  (#not-kind-eq? @_alternative "compound_statement" "if_statement")
  (#set! indent.immediate))

; Ours: an `else` starting a row is a branch, aligned with its `if`. It also
; begins, on its own row only, so the two cancel and its row keeps the
; `if`'s level.
(else_clause
  "else" @indent.branch @indent.begin
  (#set! indent.start_at_same_line))

(compound_statement
  "}" @indent.end)

[
  ")"
  "}"
  (statement_identifier)
] @indent.branch

[
  "#define"
  "#ifdef"
  "#ifndef"
  "#elif"
  "#if"
  "#else"
  "#endif"
] @indent.zero

[
  (preproc_arg)
  (string_literal)
] @indent.ignore

; The source aligns these with their opening parenthesis.
(ERROR
  (parameter_declaration)) @indent.begin

[
  (argument_list)
  (parameter_list)
] @indent.begin

(comment) @indent.auto
