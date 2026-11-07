; Adapted from tree-sitter-gleam's queries/highlights.scm at commit
; fa6d0d94804f1b342e175865a3dbeb316ee5115e (Apache-2.0, see LICENSE).
; Dropped: the `#is-not? local` predicates (and the module-access pattern that
; relied on them), the reserved-identifier and `\e` diagnostics.
; Later patterns win, so the general ones come first.

; Punctuation
[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  "<<"
  ">>"
] @punctuation.bracket
[
  "."
  ","
  ":"
  "#"
  "="
  "->"
  ".."
  "-"
  "<-"
] @punctuation.delimiter

; Variables
(identifier) @variable
(discard) @comment

; Comments
(module_comment) @comment.documentation
(statement_comment) @comment.documentation
(comment) @comment

; Constants
(constant
  name: (identifier) @constant)

; Properties: names of fields, tuple indices, labelled arguments
(label) @property
(tuple_access
  index: (integer) @property)

; Modules
(module) @module
(import alias: (identifier) @module)
(remote_type_identifier
  module: (identifier) @module)
(remote_constructor_name
  module: (identifier) @module)

; Functions
(unqualified_import (identifier) @function)
(unqualified_import "type" (type_identifier) @type)
(unqualified_import (type_identifier) @constructor)
(function
  name: (identifier) @function)
(external_function
  name: (identifier) @function)
(function_parameter
  name: (identifier) @variable.parameter)
(function_call
  function: (identifier) @function)
(binary_expression
  operator: "|>"
  right: (identifier) @function)
(function_call function: (field_access field: (label) @function))

; Attributes
(attribute
  "@" @attribute
  name: (identifier) @attribute)

(attribute_value (identifier) @constant)

; Type names
(remote_type_identifier) @type
(type_identifier) @type

; Data constructors
(constructor_name) @constructor

; Literals
(string) @string
(escape_sequence) @escape
(bit_array_segment_option) @function.builtin
(integer) @number
(float) @number

; Keywords
[
  (visibility_modifier)
  (opacity_modifier)
  "as"
  "assert"
  "case"
  "const"
  "echo"
  "external"
  "fn"
  "if"
  "import"
  "let"
  "panic"
  "todo"
  "type"
  "use"
] @keyword

; Operators
(binary_expression
  operator: _ @operator)
(boolean_negation "!" @operator)
(integer_negation "-" @operator)
