;; outline query, ours; capture convention of tree-sitter tags

; A schema-qualified name, public.t, is listed by its last part, t.
(create_table
  (object_reference name: (identifier) @name)) @definition.table

(create_view
  (object_reference name: (identifier) @name)) @definition.table

(create_materialized_view
  (object_reference name: (identifier) @name)) @definition.table

(create_type
  (object_reference name: (identifier) @name)) @definition.type

; The name comes before the arguments, with or without comments between;
; a returned type, RETURNS SETOF t, is another object_reference, after them.
(create_function
  (object_reference name: (identifier) @name)
  (function_arguments)) @definition.function
