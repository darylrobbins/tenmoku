;; imports query, ours; written for T168, capture convention: @module is the
;; string naming the imported module

(import_statement source: (string) @module)
(export_statement source: (string) @module)
; (A call with more than one argument is left out in imports.rs.)
(call_expression
  function: (identifier) @_require
  arguments: (arguments (string) @module) @arguments
  (#eq? @_require "require"))
; The grammar ends `export { … }` at a line break before `from`, or after
; `export`, and reads the module as a statement of its own. imports.rs checks
; the statements before it.
(program (expression_statement (string) @recovered))
