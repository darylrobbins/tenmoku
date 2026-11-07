; From tree-sitter-julia, https://github.com/tree-sitter/tree-sitter-julia,
; commit e0f9dcd180fdcfcfa8d79a3531e11d99e79321d3, path queries/injections.scm,
; license MIT (LICENSE). Changes: the comment and regex injections are
; dropped, as the editor has neither language.

; Inject markdown in docstrings
((string_literal
  (content) @injection.content)
  .
  [
    (module_definition)
    (abstract_definition)
    (struct_definition)
    (function_definition)
    (macro_definition)
    (assignment)
    (const_statement)
    (call_expression)
    (identifier)
  ]
  (#set! injection.language "markdown"))

; Inject markdown in md"..." and md"""...""" (e.g. md"**Bold** and _Italics_")
(prefixed_string_literal
  prefix: (identifier) @_prefix
  (content) @injection.content
  (#eq? @_prefix "md")
  (#set! injection.language "markdown"))

; Inject bash in `...` and ```...``` (e.g. `git add --help`)
(command_literal
  (content) @injection.content
  (#set! injection.language "bash"))
