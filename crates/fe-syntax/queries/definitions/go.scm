;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/go.scm in the symbols query.

; Each name of a, b int is its own field, its name alone. Embedded fields
; have no name and aren't listed.
(type_spec
  type: (struct_type
    (field_declaration_list
      (field_declaration name: (field_identifier) @name @definition.field))))

; var a, b = 1, 2 is one item named a, b, as const specs are in the outline.
(source_file
  (var_declaration
    [
      (var_spec
        name: (identifier) @name
        ("," name: (identifier) @name)*) @definition.variable
      (var_spec_list
        (var_spec
          name: (identifier) @name
          ("," name: (identifier) @name)*) @definition.variable)
    ]))
