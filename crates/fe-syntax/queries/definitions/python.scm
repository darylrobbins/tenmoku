;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/python.scm in the symbols query.

; An assignment directly in a class body, annotated or not: x = 1, x: int.
(class_definition
  body: (block
    (expression_statement
      (assignment left: (identifier) @name) @definition.field)))

; A module-level assignment to a plain name. An all-caps one is the
; outline's Constant, which comes first and wins on the same node.
(module
  (expression_statement
    (assignment left: (identifier) @name) @definition.variable))
