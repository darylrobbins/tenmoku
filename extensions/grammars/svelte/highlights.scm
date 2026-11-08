;; Svelte's highlights, for tree-sitter-svelte at
;; ae5199db47757f785e43a14b332118a5474de1a2.
;;
;; The grammar's own queries/highlights.scm (MIT, see LICENSE) starts
;; "; inherits: html", Neovim's HTML queries, so they come first here, inlined
;; from nvim-treesitter's runtime/queries/html_tags/highlights.scm and
;; runtime/queries/html/highlights.scm at
;; https://github.com/nvim-treesitter/nvim-treesitter/tree/915f2bd3d4e2a33704bfc476b5137e9befa82f3d
;; which are under the Apache license (see LICENSE-APACHE). The svelte part
;; follows. In Neovim's style, a later pattern wins.
;;
;; The differences from those files:
;; - @spell, @nospell, @none and the #set! priority and url properties are
;;   dropped, as are the text patterns for <strong>, <em>, <s> and <u>, which
;;   have no colour here.
;; - @tag.delimiter is @punctuation.bracket for HTML's angle brackets, as the
;;   built-in HTML colours them, and @punctuation.special for the #, :, / and
;;   @ opening a Svelte block or tag; an entity is @string.special.
;; - The grammar names the code in braces svelte_raw_text, not raw_text (which
;;   is a <script> or <style> element's), and its {:else if} keyword is the
;;   one token "else if": the patterns say so.
;; - A {#snippet}'s name is @function, which neither file captures.

;;; HTML (nvim-treesitter html_tags)

(tag_name) @tag

(comment) @comment

(attribute_name) @tag.attribute

(attribute
  (quoted_attribute_value) @string)

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.heading)
  (#any-of? @_tag "title" "h1" "h2" "h3" "h4" "h5" "h6"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.raw)
  (#any-of? @_tag "code" "kbd"))

((element
  (start_tag
    (tag_name) @_tag)
  (text) @markup.link)
  (#eq? @_tag "a"))

((attribute
  (attribute_name) @_attr
  (quoted_attribute_value
    (attribute_value) @string.special))
  (#any-of? @_attr "href" "src"))

[
  "<"
  ">"
  "</"
  "/>"
] @punctuation.bracket

"=" @operator

;;; HTML (nvim-treesitter html)

(doctype) @constant

"<!" @punctuation.bracket

(entity) @string.special

;;; Svelte (tree-sitter-svelte)

[
  "as"
  "key"
  "html"
  "snippet"
  "render"
] @keyword

"const" @type.qualifier

[
  "if"
  "else if"
  "else"
  "then"
] @keyword.conditional

"each" @keyword.repeat

[
  "await"
  "then"
] @keyword.coroutine

"catch" @keyword.exception

"debug" @keyword.debug

(snippet_name) @function

[
  "{"
  "}"
] @punctuation.bracket

[
  "#"
  ":"
  "/"
  "@"
] @punctuation.special
