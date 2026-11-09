; From tree-sitter-jinja, https://github.com/cathaysia/tree-sitter-jinja,
; commit f46e979278123e3b6949381896f3917f378af068, path
; tree-sitter-jinja/queries/highlights.scm, license Apache-2.0 (the grammar's
; own, in LICENSE). Neovim style: later patterns win. Changes: `@spell` and
; `@nospell` dropped (no colour for them here); "<" and ">" left out of
; `@punctuation.bracket`, since in this grammar they are only comparison
; operators; the duplicate `(endtrans_statement)` dropped; `"elif"` in place
; of `(elif_block)`, which coloured the whole branch body as a keyword; bare
; identifiers as `@variable.parameter` only in a macro's definition, not in
; every call's arguments; added patterns colour a filter's name (after `|`,
; and in `{% filter %}`, whose first filter is matched through chains of
; up to four) as a function, the word operators (`and`, `or`,
; `not`, `in`, `is`) as keywords, and a float's "." as part of the number.

[
  "{{"
  "{{-"
  "{{+"
  "+}}"
  "-}}"
  "}}"
  "{%"
  "{%-"
  "{%+"
  "+%}"
  "-%}"
  "%}"
] @keyword.directive

(string_literal) @string

(number_literal) @number

(float_literal) @number.float

(boolean_literal) @boolean

(null_literal) @constant

"defined" @constant

(comment) @comment

[
  ","
  "."
  ":"
] @punctuation.delimiter

[
  (attribute_ignore)
  (attribute_context)
  "recursive"
] @attribute.builtin

[
  "("
  ")"
  "["
  "]"
] @punctuation.bracket

(binary_operator) @operator

[
  "block"
  "with"
  "filter"
  "macro"
  "set"
  "trans"
  "pluralize"
  "autoescape"
  "required"
] @keyword

[
  (endtrans_statement)
  (endblock_statement)
  (endwith_statement)
  (endfilter_statement)
  (endmacro_statement)
  (endcall_statement)
  (endset_statement)
  (endautoescape_statement)
] @keyword

(do_statement
  "do" @keyword)

[
  "include"
  "import"
  "from"
  "extends"
  "as"
] @keyword.import

(import_statement
  (identifier) @variable)

(import_as
  (identifier) @variable)

[
  "if"
  "elif"
  (else_statement)
  (endif_statement)
] @keyword.conditional

[
  "for"
  "in"
  "continue"
  "break"
  (endfor_statement)
] @keyword.repeat

"call" @function.call

(function_call
  (identifier) @function.call)

(arg
  (identifier) @variable.parameter)

(macro_statement
  (function_call
    (arg
      (expression
        (binary_expression
          (unary_expression
            (primary_expression
              (identifier) @variable.parameter)))))))

(primary_expression
  "."
  (identifier) @variable.member)

(assignment_expression
  "."
  (identifier)+ @variable.member)

(inline_trans
  "_" @function.builtin)

"debug" @function.builtin

(raw_start) @keyword

(raw_end) @keyword

(raw_body) @markup.raw.block

(builtin_test
  [
    "boolean"
    "even"
    "in"
    "mapping"
    "sequence"
    "callable"
    "integer"
    "ne"
    "string"
    "defined"
    "filter"
    "iterable"
    "none"
    "test"
    "divisibleby"
    "float"
    "le"
    "number"
    "eq"
    "ge"
    "lower"
    "odd"
    "undefined"
    "escaped"
    "gt"
    "lt"
    "sameas"
    "upper"
  ] @keyword.operator)

; Added: a filter's name, bare (`x | upper`) or called (`x | default("-")`,
; already a call above), and the filter of a `{% filter %}` block.
(binary_expression
  (binary_operator
    "|")
  .
  (unary_expression
    (primary_expression
      (identifier) @function.call)))

(filter_statement
  (expression
    (binary_expression
      (unary_expression
        (primary_expression
          (identifier) @function.call)))))

; The grammar nests a filter chain to the left (`a | b | c` is
; ((a | b) | c)), so a `{% filter %}` block's first filter sits one level
; deeper per `|`: chains of up to four filters.
(filter_statement
  (expression
    (binary_expression
      (binary_expression
        (unary_expression
          (primary_expression
            (identifier) @function.call))))))

(filter_statement
  (expression
    (binary_expression
      (binary_expression
        (binary_expression
          (unary_expression
            (primary_expression
              (identifier) @function.call)))))))

(filter_statement
  (expression
    (binary_expression
      (binary_expression
        (binary_expression
          (binary_expression
            (unary_expression
              (primary_expression
                (identifier) @function.call))))))))

; Added: the word operators (`is`, and `in` after `for`, are each a whole
; aliased `binary_operator`).
(binary_operator
  [
    "and"
    "or"
    "not"
    "in"
  ] @keyword.operator)

((binary_operator) @keyword.operator
  (#any-of? @keyword.operator "is" "in"))

; Added: a float's "." is part of the number.
(float_literal
  "." @number.float)

(unary_operator
  "not" @keyword.operator)
