; From tree-sitter-astro at 213f6e6973d9b456c6e50e86f19f66877e7ef0ee,
; https://github.com/virchau13/tree-sitter-astro, path queries/highlights.scm,
; license MIT. Upstream's query is tree-sitter-html's, which the grammar
; extends (Neovim's version says `; inherits: html`). Changes: adds
; tree-sitter-html 0.23.2's "/>" (MIT, the grammar's own HTML base) and
; Astro's own tokens: the frontmatter fences, the braces of interpolations,
; attribute backtick strings and quoted values.

(tag_name) @tag
(erroneous_end_tag_name) @tag.error
(doctype) @constant
(attribute_name) @attribute
(attribute_value) @string
(quoted_attribute_value) @string
(attribute_backtick_string) @string
(comment) @comment

"---" @punctuation.delimiter

(html_interpolation
  [
    "{"
    "}"
  ] @punctuation.special)

(attribute_interpolation
  [
    "{"
    "}"
  ] @punctuation.special)

[
  "<"
  ">"
  "</"
  "/>"
] @punctuation.bracket
