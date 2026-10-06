;; definitions query, ours; capture convention of tree-sitter tags
; Follows javascript.scm in the symbols query.

(class_declaration
  body: (class_body
    (public_field_definition name: (_) @name) @definition.field))

(abstract_class_declaration
  body: (class_body
    (public_field_definition name: (_) @name) @definition.field))

; A class expression with a name: class Named { x = 1 }.
(class
  name: (_)
  body: (class_body
    (public_field_definition name: (_) @name) @definition.field))

(interface_declaration
  body: (interface_body
    (property_signature name: (_) @name) @definition.field))

; An object type a type alias names: type P = { x: number }.
(type_alias_declaration
  value: (object_type
    (property_signature name: (_) @name) @definition.field))

; A member without a value is its name alone.
(enum_declaration
  body: (enum_body name: (_) @name @definition.variant))

(enum_declaration
  body: (enum_body
    (enum_assignment name: (_) @name) @definition.variant))
