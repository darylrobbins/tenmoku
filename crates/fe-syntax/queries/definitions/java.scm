;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/java.scm in the symbols query.

; Each declarator of int a, b; is its own field. Fields of anonymous classes
; and enum constant bodies aren't listed.
(class_declaration
  body: (class_body
    (field_declaration
      declarator: (variable_declarator name: (identifier) @name) @definition.field)))

(record_declaration
  body: (class_body
    (field_declaration
      declarator: (variable_declarator name: (identifier) @name) @definition.field)))

(enum_declaration
  body: (enum_body
    (enum_body_declarations
      (field_declaration
        declarator: (variable_declarator name: (identifier) @name) @definition.field))))

; An interface's fields: int LIMIT = 3;.
(interface_declaration
  body: (interface_body
    (constant_declaration
      declarator: (variable_declarator name: (identifier) @name) @definition.field)))

; An annotation type's: @interface Marker { int LIMIT = 1; }.
(annotation_type_declaration
  body: (annotation_type_body
    (constant_declaration
      declarator: (variable_declarator name: (identifier) @name) @definition.field)))

(enum_constant name: (identifier) @name) @definition.variant
