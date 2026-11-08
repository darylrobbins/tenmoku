; Upstream: opa-oz/tree-sitter-nginx queries/highlights.scm at
; 7cdf006ed1addef698fc680ae31721950f85e5e9, under the grammar's MIT licence (LICENSE).
; Changed: dropped the @spell capture (no colour for it here).
; Order kept: the later pattern wins here, as in upstream's nvim-style query.

(comment) @comment

(value) @variable

(attribute (keyword) @attribute)

[
  (location_modifier)
  "="
] @operator

[
  (keyword)
  "location"
] @keyword

[
  "if"
  "map"
] @keyword.conditional

(directive (keyword) @constant)

(boolean) @boolean

[
  (auto)
  (constant)
  (level)
  (connection_method)
  (var)
  condition: (condition)
] @variable.builtin

[
  (string_literal)
  (quoted_string_literal)
  (file)
  (mask)
] @string

(directive (variable) @variable.parameter)

(directive (variable (keyword) @variable.parameter))

(location_route) @string.special
";" @punctuation.delimiter

[
  (numeric_literal)
  (time)
  (size)
  (cpumask)
] @number

[
  "{"
  "}"
] @punctuation.bracket
