;; outline query, ours; capture convention of tree-sitter tags

; A def directly in a class body is a method, decorated or not. These come
; before the plain def pattern, so they win on the same node.
(class_definition
  body: (block
    (function_definition name: (identifier) @name) @definition.method))

(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition name: (identifier) @name) @definition.method)))

(function_definition name: (identifier) @name) @definition.function

(class_definition name: (identifier) @name) @definition.class

; A module-level assignment to an all-caps name.
(module
  (expression_statement
    (assignment left: (identifier) @name) @definition.constant)
  (#match? @name "^[A-Z][A-Z0-9_]*$"))
