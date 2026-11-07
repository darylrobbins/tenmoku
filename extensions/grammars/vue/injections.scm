; Adapted from nvim-treesitter's queries, which are under the Apache license
; (LICENSE-APACHE): runtime/queries/html_tags/injections.scm, which
; vue/injections.scm inherits and is inlined first here, then
; runtime/queries/vue/injections.scm, at commit
; 915f2bd3d4e2a33704bfc476b5137e9befa82f3d.
; See https://github.com/nvim-treesitter/nvim-treesitter/tree/915f2bd3d4e2a33704bfc476b5137e9befa82f3d/runtime/queries
;
; Changes:
; - Only languages fe-syntax has built in are injected: javascript,
;   typescript, tsx, css and json. Dropped: comment, regex (<input pattern>),
;   lit-html `${}` templates (#offset!), scss for lang="less", "postcss",
;   "sass" and "scss", and pug.
; - Dropped: css in a style attribute, which the built-in CSS reads as a
;   selector (`color: red` is the tag `color` and the pseudo-class `:red`),
;   not a declaration; the value stays a string.
; - #lua-match? is #match? with the same pattern as a regular expression, and
;   #gsub! on <script type="foo/bar"> is dropped.
; - lang="tsx" and lang="jsx" set their language rather than capturing it,
;   since jsx is JavaScript's.
; - Interpolations and directive values are JavaScript, not Neovim's
;   TypeScript, which takes most template expressions the same way; a
;   directive's unquoted value is injected too.

; html_tags

; <style>...</style>
; <style blocking> ...</style>
; Without "lang" or "type", so that vue's own patterns below don't capture
; the element twice
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
      (#eq? @_attr "type")
      (quoted_attribute_value
        (attribute_value) @_type)))
  (raw_text) @injection.content)
  (#eq? @_type "importmap")
  (#set! injection.language "json"))

; <script type="module">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_attr
      (#eq? @_attr "type")
      (quoted_attribute_value
        (attribute_value) @_type)))
  (raw_text) @injection.content)
  (#eq? @_type "module")
  (#set! injection.language "javascript"))

; <input type="checkbox" onchange="this.closest('form').elements.output.value = this.checked">
(attribute
  (attribute_name) @_name
  (#match? @_name "^on[a-z]+$")
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "javascript"))

; vue

; <style lang="css">
((style_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_css)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_css "css")
  (#set! injection.language "css"))

; <script lang="js">
; <script lang="jsx">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_js)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#any-of? @_js "js" "jsx")
  (#set! injection.language "javascript"))

; <script lang="ts">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_ts)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_ts "ts")
  (#set! injection.language "typescript"))

; <script lang="tsx">
((script_element
  (start_tag
    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_tsx)))
  (raw_text) @injection.content)
  (#eq? @_lang "lang")
  (#eq? @_tsx "tsx")
  (#set! injection.language "tsx"))

; {{ expression }}
((interpolation
  (raw_text) @injection.content)
  (#set! injection.language "javascript"))

; v-if="expression", :prop="expression", @event="handler"
(directive_attribute
  (quoted_attribute_value
    (attribute_value) @injection.content)
  (#set! injection.language "javascript"))

(directive_attribute
  (attribute_value) @injection.content
  (#set! injection.language "javascript"))
