;; From tree-sitter-scss's queries/highlights.scm at commit
;; 5da5ba71a558b007b352c505bfcd9095ae337022 (MIT, see LICENSE), with the
;; tree-sitter-css 0.25.0 queries/highlights.scm (MIT, see LICENSE-CSS) that nvim-treesitter's
;; `; inherits: css` pulls in inlined ahead of it, so that a later pattern
;; wins (earlier_patterns_win = false). The @spell capture is dropped.

(comment) @comment
(js_comment) @comment

(tag_name) @tag
(nesting_selector) @tag
(universal_selector) @tag
(placeholder) @tag

"~" @operator
">" @operator
"+" @operator
"-" @operator
"*" @operator
"/" @operator
"=" @operator
"^=" @operator
"|=" @operator
"~=" @operator
"$=" @operator
"*=" @operator

"and" @operator
"or" @operator
"not" @operator
"only" @operator

[
  "=="
  "!="
  "<"
  ">"
  ">="
  "<="
] @operator

(attribute_selector (plain_value) @string)

(plain_value) @string

((plain_value) @variable
 (#match? @variable "^--"))

(class_name) @property
(id_name) @property
(namespace_name) @property
(property_name) @property
(feature_name) @property

(pseudo_element_selector (tag_name) @attribute)
(pseudo_class_selector (class_name) @attribute)
(attribute_name) @attribute

((property_name) @variable
 (#match? @variable "^(--|\\$)"))

(function_name) @function

"@media" @keyword
"@import" @keyword
"@charset" @keyword
"@namespace" @keyword
"@supports" @keyword
"@keyframes" @keyword
(at_keyword) @keyword
(to) @keyword
(from) @keyword
(important) @keyword

(string_value) @string
(color_value) @string.special

(integer_value) @number
(float_value) @number
(unit) @type

[
  "#"
  ","
  "."
  ":"
  "::"
  ";"
] @punctuation.delimiter

[
  "{"
  ")"
  "("
  "}"
] @punctuation.bracket

;; tree-sitter-scss, with the general identifier patterns first so that the
;; later, more specific ones win.

(identifier) @variable

(variable) @variable

[
  "@at-root"
  "@debug"
  "@error"
  "@extend"
  "@forward"
  "@mixin"
  "@use"
  "@warn"
  "@if"
  "@else"
] @keyword

(else_if_clause
  "if" @keyword)

"@function" @keyword.function

"@return" @keyword

"@include" @keyword

[
  "@while"
  "@each"
  "@for"
  "from"
  "through"
  "in"
] @keyword

(mixin_statement
  name: (identifier) @function)

(mixin_statement
  (parameters
    (parameter) @variable.parameter))

(function_statement
  name: (identifier) @function)

(function_statement
  (parameters
    (parameter) @variable.parameter))

(keyword_query) @function

(argument) @variable.parameter

(arguments
  (variable) @variable.parameter)

[
  "["
  "]"
] @punctuation.bracket

(include_statement
  (identifier) @function)
