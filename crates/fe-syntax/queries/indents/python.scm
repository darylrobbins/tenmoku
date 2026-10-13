; Indent query for Python, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/python/indents.scm, license Apache-2.0.
; Changes: we don't support `@indent.align`, so the brackets the source
; aligns begin instead, and its aligned conditions, which those cover, are
; left out. The source ends a `return`'s region after its last node; here
; `return`, `pass` and `raise` dedent the rows after them, as `break` and
; `continue` do, set immediate so that's after their last row, not inside
; them. `#lua-match?` becomes `#match?` with the same pattern.

[
  (import_from_statement)
  (generator_expression)
  (list_comprehension)
  (set_comprehension)
  (dictionary_comprehension)
  (tuple_pattern)
  (list_pattern)
  (binary_operator)
  (lambda)
  (concatenated_string)
] @indent.begin

; The source aligns these with their opening bracket.
[
  (list)
  (dictionary)
  (set)
  (parenthesized_expression)
  (argument_list)
  (parameters)
  (tuple)
] @indent.begin

((for_statement) @indent.begin
  (#set! indent.immediate 1))

((if_statement) @indent.begin
  (#set! indent.immediate 1))

((while_statement) @indent.begin
  (#set! indent.immediate 1))

((try_statement) @indent.begin
  (#set! indent.immediate 1))

(ERROR
  "try"
  .
  ":"
  (#set! indent.immediate 1)) @indent.begin

(ERROR
  "try"
  .
  ":"
  (ERROR
    (block
      (expression_statement
        (identifier) @_except) @indent.branch))
  (#eq? @_except "except"))

((function_definition) @indent.begin
  (#set! indent.immediate 1))

((class_definition) @indent.begin
  (#set! indent.immediate 1))

((with_statement) @indent.begin
  (#set! indent.immediate 1))

((match_statement) @indent.begin
  (#set! indent.immediate 1))

((case_clause) @indent.begin
  (#set! indent.immediate 1))

; The source aligns these with the bracket, when something follows it.
(ERROR
  [
    "("
    "["
    "{"
  ] @indent.begin
  (#set! indent.immediate 1))

([
  (break_statement)
  (continue_statement)
  (return_statement)
  (pass_statement)
  (raise_statement)
] @indent.dedent
  (#set! indent.immediate))

(ERROR
  (_) @indent.branch
  ":"
  .
  (#match? @indent.branch "^else"))

(ERROR
  (_) @indent.branch @indent.dedent
  ":"
  .
  (#match? @indent.branch "^elif"))

(generator_expression
  ")" @indent.end)

(list_comprehension
  "]" @indent.end)

(set_comprehension
  "}" @indent.end)

(dictionary_comprehension
  "}" @indent.end)

(tuple_pattern
  ")" @indent.end)

(list_pattern
  "]" @indent.end)

[
  ")"
  "]"
  "}"
  (elif_clause)
  (else_clause)
  (except_clause)
  (finally_clause)
] @indent.branch

(string) @indent.auto
