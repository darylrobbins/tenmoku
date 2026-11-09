; From tree-sitter-heex at 5842537f734d7c12685bf27d6005313e3e5a47a0,
; https://github.com/phoenixframework/tree-sitter-heex, path
; queries/highlights.scm, license MIT. Changes: drops `(ERROR) @error` (no
; such highlight here); the rest is upstream's, in its order.

; General HEEx delimiters
[
  "<"
  "<!"
  "</"
  "<:"
  "</:"
  ">"
  "/>"
  "{"
  "}"
] @punctuation.bracket

; HEEx directive expressions can use the following tag delimiters:
;     `<%` and `%>`
;     `<%=` and `%>`
;     `<%%` and `%>`
;     `<%%=` and `%>`
(directive) @keyword

; HEEx comments can use the following tag delimiters:
;     `<!--` and `-->`
;     `<%!--` and `--%>`
;     `<%#` and `%>`
(comment) @comment

; HEEx operators are highlighted as such
"=" @operator

; HEEx entities are highligted as special strings
(entity) @string.special

; HEEx inherits the DOCTYPE tag from HTML
(doctype) @constant

; HEEx tags and slots are highlighted as HTML tags
[
  (tag_name)
  (slot_name)
] @tag

; HEEx components are highlighted as Elixir modules and functions
(component_name
  [
    (module) @module
    (function) @function
    "." @punctuation.delimiter
  ])

; HEEx attributes are highlighted as HTML attributes
(attribute_name) @attribute

; HEEx special attributes can be any of the following:
;     `:let`
;     `:if`
;     `:for`
;     `:key`
;     `:stream`
(special_attribute_name) @keyword

; HEEx attribute values are highlighted as HTML attribute values
[
  (attribute_value)
  (quoted_attribute_value)
] @string
