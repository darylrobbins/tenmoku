; From tree-sitter-go-template at 06b2fd2ac32a7be71cdcf925eeeb868a68e2acde,
; https://github.com/ngalaiko/tree-sitter-go-template, path
; queries/highlights.scm, license MIT. Changes: drops `(ERROR) @error` (not a
; highlight name here) and the duplicated `ge` in the builtin list.

; Identifiers

[
    (field)
    (field_identifier)
] @property

(variable) @variable

; Function calls

(function_call
  function: (identifier) @function)

(method_call
  method: (selector_expression
    field: (field_identifier) @function))

; Operators

"|" @operator
":=" @operator

; Builtin functions

((identifier) @function.builtin
 (#match? @function.builtin "^(and|call|html|index|slice|js|len|not|or|print|printf|println|urlquery|eq|ne|lt|ge|gt)$"))

; Delimiters

"." @punctuation.delimiter
"," @punctuation.delimiter

"{{" @punctuation.bracket
"}}" @punctuation.bracket
"{{-" @punctuation.bracket
"-}}" @punctuation.bracket
")" @punctuation.bracket
"(" @punctuation.bracket

; Keywords

"else" @keyword
"if" @keyword
"range" @keyword
"with" @keyword
"end" @keyword
"template" @keyword
"define" @keyword
"block" @keyword

; Literals

[
  (interpreted_string_literal)
  (raw_string_literal)
  (rune_literal)
] @string

(escape_sequence) @string.special

[
  (int_literal)
  (float_literal)
  (imaginary_literal)
] @number

[
  (true)
  (false)
  (nil)
] @constant.builtin

(comment) @comment
