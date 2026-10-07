;; definitions query, ours, written from tree-sitter-swift 0.7.3's
;; node-types.json; capture convention of tree-sitter tags
; Follows outline/swift.scm in the symbols query.

; Stored and computed properties of a type or an extension, static or not.
; Each name of let a = 1, b = 2 is its own item, the pattern naming it.
[
  (class_body
    (property_declaration
      name: (pattern bound_identifier: (simple_identifier) @name) @definition.field))
  (enum_class_body
    (property_declaration
      name: (pattern bound_identifier: (simple_identifier) @name) @definition.field))
]

(protocol_body
  (protocol_property_declaration
    name: (pattern bound_identifier: (simple_identifier) @name) @definition.field))

; case south, east: each name is its own item.
(enum_entry name: (simple_identifier) @name @definition.variant)

; let limit = 10 at the top of the file. One in a function body is a local.
(source_file
  (property_declaration
    name: (pattern bound_identifier: (simple_identifier) @name) @definition.variable))

; let (a, (b, c)) = pair there, and in a type's body: each name, at any
; depth, is its own item. See #bound? in outline.rs.
((pattern (pattern (simple_identifier) @name @definition.variable))
  (#bound? @name source_file))

((pattern (pattern (simple_identifier) @name @definition.field))
  (#bound? @name class_body enum_class_body))
