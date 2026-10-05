;; outline query, ours; capture convention of tree-sitter tags
; TypeScript and TSX use this file too, followed by typescript.scm.

(class_declaration name: (_) @name) @definition.class

(method_definition name: (_) @name) @definition.method

(function_declaration name: (_) @name) @definition.function

(generator_function_declaration name: (_) @name) @definition.function

; const, let or var bound to an arrow function or function expression.
(variable_declarator
  name: (identifier) @name
  value: [(arrow_function) (function_expression)]) @definition.function
