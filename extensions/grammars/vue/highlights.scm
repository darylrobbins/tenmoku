; Adapted from nvim-treesitter's queries, which are under the Apache license
; (LICENSE-APACHE): runtime/queries/html_tags/highlights.scm, which
; vue/highlights.scm inherits and is inlined first here, then
; runtime/queries/vue/highlights.scm, at commit
; 915f2bd3d4e2a33704bfc476b5137e9befa82f3d.
; See https://github.com/nvim-treesitter/nvim-treesitter/tree/915f2bd3d4e2a33704bfc476b5137e9befa82f3d/runtime/queries
;
; Later patterns win, as in Neovim. Changes:
; - Dropped what fe-syntax has no colour or support for: @spell and @nospell,
;   @none on text, the markup.strong, italic, strikethrough and underline
;   patterns, `#set! priority`, the url property and Neovim's commentstring.
; - An interpolation's {{ and }}, and a directive value's quotes, are
;   punctuation.special, rather than the whole node with @none on its inside:
;   the inside is JavaScript, injected (injections.scm).
; - Tag delimiters are punctuation.bracket, as the built-in HTML's are, and a
;   directive's `:` and `.` punctuation.delimiter (Neovim's
;   character.special); `#`, v-slot's shorthand, is punctuation.special as `@`
;   is.
; - A doctype, an erroneous end tag and an unquoted attribute value are
;   coloured, as the built-in HTML colours them.

; html_tags

(tag_name) @tag

(erroneous_end_tag_name) @tag

(comment) @comment

(attribute_name) @tag.attribute

(attribute
  (quoted_attribute_value) @string)

(attribute
  (attribute_value) @string)

(doctype) @constant

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading)
  (#eq? @_tag "title"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.1)
  (#eq? @_tag "h1"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.2)
  (#eq? @_tag "h2"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.3)
  (#eq? @_tag "h3"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.4)
  (#eq? @_tag "h4"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.5)
  (#eq? @_tag "h5"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading.6)
  (#eq? @_tag "h6"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.raw)
  (#any-of? @_tag "code" "kbd"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.link.label)
  (#eq? @_tag "a"))

((attribute
  (attribute_name) @_attr
  (quoted_attribute_value
    (attribute_value) @string.special.url))
  (#any-of? @_attr "href" "src"))

[
  "<"
  ">"
  "</"
  "/>"
  "<!"
] @punctuation.bracket

"=" @operator

; vue

[
  "["
  "]"
] @punctuation.bracket

[
  ":"
  "."
] @punctuation.delimiter

[
  "{{"
  "}}"
  "@"
  "#"
] @punctuation.special

(dynamic_directive_inner_value) @variable

(directive_name) @tag.attribute

; Accessing a component object's field
(":"
  .
  (directive_value) @variable.member)

("."
  .
  (directive_value) @property)

; @click is like onclick for HTML
("@"
  .
  (directive_value) @function.method)

; Used in v-slot, declaring position the element should be put in
("#"
  .
  (directive_value) @variable)

(directive_attribute
  (quoted_attribute_value
    "\"" @punctuation.special))

(directive_attribute
  (quoted_attribute_value
    "'" @punctuation.special))

(directive_modifier) @function.method
