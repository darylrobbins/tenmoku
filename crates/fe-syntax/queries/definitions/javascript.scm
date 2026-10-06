;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/javascript.scm in the symbols query. TypeScript and TSX use
; this file too, followed by typescript.scm.

; A field of a declared class, or of a class expression with a name: x = 1,
; #y. An anonymous class expression's aren't listed. TypeScript's grammar
; has no field_definition node, which only JavaScript's class bodies hold
; with a property field, so the wildcard keeps this valid for both.
(class_declaration
  body: (class_body
    (_ property: (_) @name) @definition.field))

(class
  name: (_)
  body: (class_body
    (_ property: (_) @name) @definition.field))

; const, let or var directly in the program or an export. One bound to a
; function, also a generator or one in parentheses, isn't a variable: the
; outline lists those bound to an arrow function or function expression as
; a Function.
(program
  [
    (lexical_declaration
      (variable_declarator name: (identifier) @name) @definition.variable)
    (variable_declaration
      (variable_declarator name: (identifier) @name) @definition.variable)
  ]
  (#non-function? @definition.variable))

(program
  (export_statement
    declaration: [
      (lexical_declaration
        (variable_declarator name: (identifier) @name) @definition.variable)
      (variable_declaration
        (variable_declarator name: (identifier) @name) @definition.variable)
    ])
  (#non-function? @definition.variable))

; Each name a destructuring declaration there binds, at any depth:
; const { a, b: c, ...d } = o; lists a, c and d, but not the key b.
([
  (object_pattern (shorthand_property_identifier_pattern) @name @definition.variable)
  (object_assignment_pattern
    left: (shorthand_property_identifier_pattern) @name @definition.variable)
  (pair_pattern value: (identifier) @name @definition.variable)
  (array_pattern (identifier) @name @definition.variable)
  (assignment_pattern left: (identifier) @name @definition.variable)
  (rest_pattern (identifier) @name @definition.variable)
 ]
 (#destructured? @name))
