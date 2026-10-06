;; definitions query, adapted from tree-sitter-kotlin-sg 0.4.1
;; queries/tags.scm (MIT); capture convention of tree-sitter tags
; Follows outline/kotlin.scm in the symbols query.
; Changed: properties are fields in a class, object or interface body and
; variables at the top of the file, rather than constants anywhere, so a
; local val isn't listed; enum entries are variants; properties declared
; in a primary constructor are added.

[
  (class_body
    (property_declaration
      (variable_declaration (simple_identifier) @name)) @definition.field)
  (enum_class_body
    (property_declaration
      (variable_declaration (simple_identifier) @name)) @definition.field)
]

; class Circle(private val radius: Double): val and var parameters declare
; properties. Plain parameters, as in class C(x: Int), don't.
; The name follows val or var; an identifier in a default value, as seed in
; val x: Int = seed, isn't one.
(class_parameter (binding_pattern_kind) . (simple_identifier) @name) @definition.field

(enum_entry (simple_identifier) @name) @definition.variant

(source_file
  (property_declaration
    (variable_declaration (simple_identifier) @name)) @definition.variable)

; val (a, b) = pair at the top: each name is its own item.
(source_file
  (property_declaration
    (multi_variable_declaration
      (variable_declaration (simple_identifier) @name) @definition.variable)))
