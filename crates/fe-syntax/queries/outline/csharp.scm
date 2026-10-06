;; outline query, ours, written from tree-sitter-c-sharp 0.23.5's
;; node-types.json; capture convention of tree-sitter tags

(namespace_declaration name: (_) @name) @definition.module

; namespace Shop; lists only the statement: the grammar doesn't make the
; rest of the file its body.
(file_scoped_namespace_declaration name: (_) @name) @definition.module

(class_declaration name: (identifier) @name) @definition.class

; A record is a class, as a record struct is: record struct P(int X);.
(record_declaration name: (identifier) @name) @definition.class

(struct_declaration name: (identifier) @name) @definition.struct

(interface_declaration name: (identifier) @name) @definition.interface

(enum_declaration name: (identifier) @name) @definition.enum

(delegate_declaration name: (identifier) @name) @definition.type

(method_declaration name: (identifier) @name) @definition.method

(constructor_declaration name: (identifier) @name) @definition.method

; ~Order().
(destructor_declaration "~" @name name: (identifier) @name) @definition.method

; operator +.
(operator_declaration "operator" @name operator: _ @name) @definition.method

; implicit operator int.
(conversion_operator_declaration "operator" @name type: (_) @name) @definition.method

; An indexer is named this, as it's declared: public int this[int i].
(indexer_declaration "this" @name) @definition.method

; A const field is a constant, each declarator its own item. It comes
; before the definitions query's field pattern, so it wins on the same
; node.
(field_declaration
  (modifier "const")
  (variable_declaration
    (variable_declarator name: (identifier) @name) @definition.constant))
