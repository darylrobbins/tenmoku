;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/sql.scm in the symbols query.

; A table's columns, which nest under it. A quoted name, "when", is a
; literal there.
(create_table
  (column_definitions
    (column_definition name: [(identifier) (literal)] @name) @definition.field))
