; From tree-sitter-astro at 213f6e6973d9b456c6e50e86f19f66877e7ef0ee,
; https://github.com/virchau13/tree-sitter-astro, path queries/injections.scm,
; license MIT. Changes: drops the injection of `<style lang="scss">` (no SCSS
; grammar here) and injects every `<style>` as CSS instead, as
; tree-sitter-html 0.23.2's queries/injections.scm (MIT) does; a `<script>`
; stays TypeScript, which Astro compiles scripts as.

; The frontmatter between the `---` fences.
(frontmatter
  (frontmatter_js_block) @injection.content
  (#set! injection.language "typescript"))

; `attr={expression}` and `{...spread}`.
(attribute_interpolation
  (attribute_js_expr) @injection.content
  (#set! injection.language "typescript"))

; The expression text of `{...}` among elements.
(html_interpolation
  (permissible_text) @injection.content
  (#set! injection.language "typescript"))

(script_element
  (raw_text) @injection.content
  (#set! injection.language "typescript"))

(style_element
  (raw_text) @injection.content
  (#set! injection.language "css"))
