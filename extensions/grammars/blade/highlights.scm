; From tree-sitter-blade at b5291d1ba207a8ebb8383b2ecb8a8a6535210a50,
; https://github.com/EmranMR/tree-sitter-blade, path queries/highlights.scm,
; license MIT. Upstream's query starts `; inherits: html`: in its place come
; tree-sitter-html 0.23.2's queries/highlights.scm (MIT,
; https://github.com/tree-sitter/tree-sitter-html), whose grammar Blade's
; extends, unchanged. Changes: that inlined HTML query, and Blade's
; quoted_attribute_value as @string (as tree-sitter-astro's query has it), so
; the quotes around a value colour with it.

; tree-sitter-html 0.23.2, queries/highlights.scm.

(tag_name) @tag
(erroneous_end_tag_name) @tag.error
(doctype) @constant
(attribute_name) @attribute
(attribute_value) @string
(comment) @comment

[
  "<"
  ">"
  "</"
  "/>"
] @punctuation.bracket

; Added: the quotes around an attribute's value.

(quoted_attribute_value) @string

; tree-sitter-blade, queries/highlights.scm.

[
  (directive)
  (directive_start)
  (directive_end)
] @tag

[
  (php_tag)
  (php_end_tag)
  "{{"
  "}}"
  "{!!"
  "!!}"
  "("
  ")"
] @punctuation.bracket
