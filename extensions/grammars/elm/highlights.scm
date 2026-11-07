; Adapted from tree-sitter-elm's queries/highlights.scm at commit
; e1e8fea161a1e66f3997855d316be2a43e4e956f (MIT, the grammar's licence in
; LICENSE): captures renamed to fe-syntax's vocabulary.

; Keywords
[
  "if"
  "then"
  "else"
  "let"
  "in"
] @keyword
(case) @keyword
(of) @keyword

(backslash) @keyword
(as) @keyword
(port) @keyword
(exposing) @keyword
(alias) @keyword
(infix) @keyword
(module) @keyword
(import) @keyword
(type) @keyword
"|" @keyword

(colon) @punctuation.delimiter
(arrow) @operator
(operator_identifier) @operator
(eq) @operator

"(" @punctuation.bracket
")" @punctuation.bracket
"," @punctuation.delimiter

; Functions
(type_annotation (lower_case_identifier) @function)
(port_annotation (lower_case_identifier) @function)
(function_declaration_left (lower_case_identifier) @function)
(function_call_expr target: (value_expr) @function)

(lower_pattern) @variable.parameter
(record_base_identifier) @variable

; Numbers
(number_constant_expr) @number

; Types
(type_declaration (upper_case_identifier) @type)
(type_ref) @type
(type_alias_declaration name: (upper_case_identifier) @type)

(union_variant (upper_case_identifier) @constructor)
(union_pattern) @constructor
(value_expr (upper_case_qid (upper_case_identifier)) @constructor)

; Comments
(line_comment) @comment
(block_comment) @comment

; Strings
(string_escape) @escape
(open_quote) @string
(close_quote) @string
(regular_string_part) @string
(open_char) @string
(close_char) @string
