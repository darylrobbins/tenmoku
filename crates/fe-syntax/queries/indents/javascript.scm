; Indent query for JavaScript, TypeScript and TSX, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6, license Apache-2.0,
; path runtime/queries/ecma/indents.scm (which javascript/indents.scm
; inherits). JavaScript's query is this and `jsx.scm`, TypeScript's this and
; `typescript.scm`, TSX's all three.
; Changes: `indent.rs` indents between brackets itself, from the opener's
; row, so `switch_statement` no longer begins at its first row, and a `{`
; (or an object argument) is no longer a branch: either put a `{` on a row of
; its own out of line. Switch cases begin immediately, as their bodies have
; no closer. The source's unbraced `if` body, which begins the whole `if`
; and dedents the body, is replaced by the body patterns at the end, which
; with the loop, `do`, `;` and `else` patterns there and the `break` pattern
; are ours.

[
  (arguments)
  (array)
  (binary_expression)
  (class_body)
  (export_clause)
  (formal_parameters)
  (named_imports)
  (object)
  (object_pattern)
  (parenthesized_expression)
  (return_statement)
  (statement_block)
  (template_substitution)
  (ternary_expression)
] @indent.begin

([
  (switch_case)
  (switch_default)
] @indent.begin
  (#set! indent.immediate))

; Ours: a case's `break` ends it.
(switch_case
  (break_statement) @indent.dedent
  (#set! indent.immediate))

(arguments
  (call_expression) @indent.begin)

(binary_expression
  (call_expression) @indent.begin)

(expression_statement
  (call_expression) @indent.begin)

(arrow_function
  body: (_) @_body
  (#not-kind-eq? @_body "statement_block")) @indent.begin

(assignment_expression
  right: (_) @_right
  (#not-kind-eq? @_right "arrow_function")) @indent.begin

(variable_declarator
  value: (_) @_value
  (#not-kind-eq? @_value "arrow_function" "call_expression")) @indent.begin

(arguments
  ")" @indent.end)

(object
  "}" @indent.end)

(statement_block
  "}" @indent.end)

[
  ")"
  "}"
  "]"
] @indent.branch

((parenthesized_expression
  "("
  (_)
  ")" @indent.end) @_outer
  (#not-has-parent? @_outer if_statement))

[
  "}"
  "]"
] @indent.end

(template_string) @indent.ignore

[
  (comment)
  (ERROR)
] @indent.auto

; Ours: a loop with an unbraced body begins, as C's do.
([
  (for_statement
    body: (_) @_body)
  (for_in_statement
    body: (_) @_body)
  (while_statement
    body: (_) @_body)
] @indent.begin
  (#not-kind-eq? @_body "statement_block"))

; Ours: an unbraced `do` body is a level in from the `do`, and so is a new
; row after the `do` it follows; the `while` lines up with the `do`.
(do_statement
  body: (_) @indent.begin
  (#not-kind-eq? @indent.begin "statement_block")
  (#set! indent.start_at_same_line))

(do_statement
  "do" @indent.begin
  body: (_) @_body
  (#not-kind-eq? @_body "statement_block")
  (#set! indent.immediate))

; Ours: an unbraced body, on the `if`'s row or its own, is a level in, and
; so is a new row after the condition or `else` it follows.
(if_statement
  consequence: (_) @indent.begin
  (#not-kind-eq? @indent.begin "statement_block")
  (#set! indent.start_at_same_line))

(if_statement
  condition: (parenthesized_expression
    ")" @indent.begin)
  consequence: (_) @_consequence
  (#not-kind-eq? @_consequence "statement_block")
  (#set! indent.immediate))

(else_clause
  (_) @indent.begin
  (#not-kind-eq? @indent.begin "statement_block" "if_statement")
  (#set! indent.start_at_same_line))

(else_clause
  "else" @indent.begin
  (_) @_alternative
  (#not-kind-eq? @_alternative "statement_block" "if_statement")
  (#set! indent.immediate))

; Ours: an `else` starting a row is a branch, aligned with its `if`. It also
; begins, on its own row only, so the two cancel and its row keeps the
; `if`'s level.
(else_clause
  "else" @indent.branch @indent.begin
  (#set! indent.start_at_same_line))

; Ours: a statement's `;` ends it, so the row after a multi-row statement,
; such as a `return` or a declaration that begins, isn't indented.
";" @indent.end
