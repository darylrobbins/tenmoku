; Upstream: tree-sitter-grammars/tree-sitter-kdl queries/highlights.scm at
; e8ff98a113e175b84b92ec2096820517c1e10a66, under the grammar's MIT licence (LICENSE).
; Changed: node names are @function (KDL has no keywords or functions; a node
; name is the closest thing to a command), later patterns win here as upstream's
; highlighter takes the last. Dropped: the #any-of? type.builtin pattern (a
; predicate fe-syntax doesn't apply, so it would colour every type) and @spell.

; Variables

(identifier) @variable

; Operators

"=" @operator

; Literals

(string) @string

(multi_line_string) @string

(string_fragment) @string

(escape) @escape

(escaped_whitespace) @escape

(number) @number

(keyword_number) @number

(number (decimal) @number)
(number (exponent) @number)

(boolean) @constant.builtin

[ "null" "#null" ] @constant.builtin

; Punctuation

["{" "}"] @punctuation.bracket

["(" ")"] @punctuation.bracket

[
  ";"
] @punctuation.delimiter

; Comments

[
  (single_line_comment)
  (multi_line_comment)
  (version)
] @comment

(node_comment) @comment
(node_field_comment) @comment
(node_children_comment) @comment

; Contextual names come after the generic string captures so that a quoted
; node name, property key or type name keeps its semantic colour, fragments too.

(node name: (identifier) @function)
(node name: (identifier (string) @function))
(node name: (identifier (string (string_fragment) @function)))

(type) @type
(type name: (identifier) @type)
(type name: (identifier (string) @type))
(type name: (identifier (string (string_fragment) @type)))

(prop key: (identifier) @property)
(prop key: (identifier (string) @property))
(prop key: (identifier (string (string_fragment) @property)))
