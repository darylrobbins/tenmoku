;; definitions query, ours, written from tree-sitter-c-sharp 0.23.5's
;; node-types.json; capture convention of tree-sitter tags
; Follows outline/csharp.scm in the symbols query.

; Fields and events of a type, each declarator its own item. A local
; variable is a variable_declaration in a local_declaration_statement and
; doesn't match.
(field_declaration
  (variable_declaration
    (variable_declarator name: (identifier) @name) @definition.field))

(event_field_declaration
  (variable_declaration
    (variable_declarator name: (identifier) @name) @definition.field))

(property_declaration name: (identifier) @name) @definition.field

(event_declaration name: (identifier) @name) @definition.field

; A record's positional parameters declare its properties:
; record Receipt(int Number) has a property Number. A class's or struct's
; primary constructor parameters are parameters, and aren't listed.
(record_declaration
  (parameter_list
    (parameter name: (identifier) @name) @definition.field))

(enum_member_declaration name: (identifier) @name) @definition.variant
