; Goes after tree-sitter-kotlin-sg's highlights.scm for Kotlin.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 3e1f54c1ce0a86959313f1cb7f60262ea57aaa23,
; path runtime/queries/kotlin/highlights.scm, license Apache-2.0.
; Changes: only the patterns below are taken, and `#lua-match?` becomes
; `#match?` with the same pattern.
;
; The crate's query is an older copy of the same file. It anchors a function's
; name to the first child, so annotated or modified functions (`private fun f`)
; lost it, it colours the whole `package` line as a module, and it has no
; `companion` keyword or SCREAMING_CASE constants. Later patterns win over
; earlier ones on the same node.

((simple_identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

(_
  (navigation_suffix
    (simple_identifier) @constant
    (#match? @constant "^[A-Z][A-Z0-9_]*$")))

(package_header
  "package" @keyword
  .
  (identifier
    (simple_identifier) @module))

(function_declaration
  (simple_identifier) @function)

(companion_object
  "companion" @keyword)
