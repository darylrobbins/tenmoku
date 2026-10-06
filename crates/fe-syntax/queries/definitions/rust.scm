;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/rust.scm in the symbols query.

; Named fields only: a tuple struct's are numbered.
(struct_item
  body: (field_declaration_list
    (field_declaration name: (field_identifier) @name) @definition.field))

(union_item
  body: (field_declaration_list
    (field_declaration name: (field_identifier) @name) @definition.field))

(enum_item
  body: (enum_variant_list
    (enum_variant name: (identifier) @name) @definition.variant))
