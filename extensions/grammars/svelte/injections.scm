;; Svelte's injections, for tree-sitter-svelte at
;; ae5199db47757f785e43a14b332118a5474de1a2.
;;
;; The grammar's own queries/injections.scm (MIT, see LICENSE) starts
;; "; inherits: html_tags", so that file comes first here, inlined from
;; nvim-treesitter's runtime/queries/html_tags/injections.scm at
;; https://github.com/nvim-treesitter/nvim-treesitter/tree/915f2bd3d4e2a33704bfc476b5137e9befa82f3d
;; which is under the Apache license (see LICENSE-APACHE). The svelte part
;; follows, with the <script lang="js"> pattern from nvim-treesitter's
;; runtime/queries/svelte/injections.scm at the same commit.
;;
;; The differences from those files:
;; - Only languages the editor has are injected: not comment, regex, scss
;;   (for <style lang="scss">, "postcss", "less" or "stylus") or pug.
;; - A style="..." attribute isn't injected, as the built-in HTML doesn't
;;   inject it: CSS's grammar parses a stylesheet, where a bare declaration
;;   list such as "font-weight: bold" reads as a selector.
;; - #lua-match? and #not-lua-match? are #match? and #not-match? with the
;;   same patterns as regular expressions. The patterns needing #gsub! (a
;;   <script type> naming its language) or #offset! (lit-html's ${...}) are
;;   dropped.
;; - The grammar names the code in braces svelte_raw_text, not raw_text
;;   (which is a <script> or <style> element's): the pattern says so.

;;; HTML (nvim-treesitter html_tags)

; <style>...</style>
; <style blocking> ...</style>
; "lang" is in the predicate so that a <style lang="..."> isn't taken twice.
((style_element
  (start_tag) @_no_type_lang
  (raw_text) @injection.content)
  (#not-match? @_no_type_lang "\\slang\\s*=")
  (#not-match? @_no_type_lang "\\stype\\s*=")
  (#set! injection.language "css"))

((style_element
  (start_tag
    (attribute
      (attribute_name) @_type
      (quoted_attribute_value
        (attribute_value) @_css)))
  (raw_text) @injection.content)
  (#eq? @_type "type")
  (#eq? @_css "text/css")
  (#set! injection.language "css"))

; <script>...</script>
; <script defer>...</script>
((script_element
  (start_tag) @_no_type_lang
  (raw_text) @injection.content)
  (#not-match? @_no_type_lang "\\slang\\s*=")
  (#not-match? @_no_type_lang "\\stype\\s*=")
  (#set! injection.language "javascript"))

; <script type="importmap">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value
        (attribute_value) @_type)))
  (raw_text) @injection.content)
  (#eq? @_attr "type")
  (#eq? @_type "importmap")
  (#set! injection.language "json"))

; <script type="module">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value
        (attribute_value) @_type)))
  (raw_text) @injection.content)
  (#eq? @_attr "type")
  (#eq? @_type "module")
  (#set! injection.language "javascript"))

; <input type="checkbox" onchange="this.closest('form').elements.output.value = this.checked">
((attribute
  (attribute_name) @_name
  (quoted_attribute_value
    (attribute_value) @injection.content))
  (#match? @_name "^on[a-z]+$")
  (#set! injection.language "javascript"))

;;; Svelte (tree-sitter-svelte)

; {expression}, {#if condition}, {#each items as item} ...
((svelte_raw_text) @injection.content
  (#set! injection.language "javascript"))

; <script lang="ts">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value
        (attribute_value) @_lang)))
  (raw_text) @injection.content)
  (#eq? @_attr "lang")
  (#any-of? @_lang "ts" "typescript")
  (#set! injection.language "typescript"))

; <script lang="js">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value
        (attribute_value) @_lang)))
  (raw_text) @injection.content)
  (#eq? @_attr "lang")
  (#any-of? @_lang "js" "javascript")
  (#set! injection.language "javascript"))
