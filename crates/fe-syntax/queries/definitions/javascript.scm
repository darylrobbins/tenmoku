;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/javascript.scm in the symbols query. TypeScript and TSX use
; this file too, followed by typescript.scm.

; A field of a declared class: x = 1, #y. TypeScript's grammar has no
; field_definition node, which only JavaScript's class bodies hold with a
; property field, so the wildcard keeps this valid for both.
(class_declaration
  body: (class_body
    (_ property: (_) @name) @definition.field))

; const, let or var directly in the program or an export. One bound to a
; function is the outline's Function, which comes first and wins on the
; same node.
(program
  [
    (lexical_declaration
      (variable_declarator name: (identifier) @name) @definition.variable)
    (variable_declaration
      (variable_declarator name: (identifier) @name) @definition.variable)
  ])

(program
  (export_statement
    declaration: [
      (lexical_declaration
        (variable_declarator name: (identifier) @name) @definition.variable)
      (variable_declaration
        (variable_declarator name: (identifier) @name) @definition.variable)
    ]))
