; Adapted from nvim-treesitter's runtime/queries/hcl/highlights.scm and
; runtime/queries/terraform/highlights.scm (which inherits hcl), at commit
; 915f2bd3d4e2a33704bfc476b5137e9befa82f3d
; (https://github.com/nvim-treesitter/nvim-treesitter), Apache-2.0: the text is
; in LICENSE-APACHE. Rewritten for this grammar's node names and fe-syntax's
; captures; patterns using unsupported predicates are dropped.

[
  "if"
  "else"
  "endif"
] @keyword.conditional

[
  "for"
  "endfor"
  "in"
] @keyword.repeat

(conditional "?" @keyword.conditional ":" @keyword.conditional)

[
  (bool_lit)
  (null_lit)
] @constant.builtin

(numeric_lit) @number

(comment) @comment

(identifier) @variable

(body (block (identifier) @keyword))
(body (block (body (block (identifier) @type))))
(function_call (identifier) @function)
(attribute (identifier) @property)
(object_elem key: (expression (variable_expr (identifier) @property)))
(get_attr (identifier) @property)

((variable_expr (identifier) @variable.builtin)
  (#any-of? @variable.builtin "var" "local" "module" "data" "path" "terraform" "each" "count" "self"))

((variable_expr (identifier) @type.builtin)
  (#any-of? @type.builtin "string" "number" "bool" "any"))

((function_call (identifier) @type.builtin)
  (#any-of? @type.builtin "list" "map" "set" "object" "tuple" "optional"))

(block (string_lit) @string.special)

(string_lit) @string
(quoted_template_start) @string
(quoted_template_end) @string
(template_literal) @string

[
  (heredoc_start)
  (heredoc_identifier)
] @string.special

[
  (template_interpolation_start)
  (template_interpolation_end)
  (template_directive_start)
  (template_directive_end)
  (strip_marker)
] @punctuation.special

[
  "="
  "+"
  "-"
  "*"
  "/"
  "%"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "&&"
  "||"
  "!"
  "=>"
] @operator

[
  "."
  ","
  ":"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket
