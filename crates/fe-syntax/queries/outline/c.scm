;; outline query, ours; capture convention of tree-sitter tags

; Through up to two pointer declarators: int *f(void), char **g(void).
(function_definition
  declarator: [
    (function_declarator declarator: (identifier) @name)
    (pointer_declarator
      declarator: (function_declarator declarator: (identifier) @name))
    (pointer_declarator
      declarator: (pointer_declarator
        declarator: (function_declarator declarator: (identifier) @name)))
  ]) @definition.function

; Only with a body: struct foo *p; isn't a definition.
(struct_specifier name: (type_identifier) @name body: (_)) @definition.struct

(union_specifier name: (type_identifier) @name body: (_)) @definition.struct

(enum_specifier name: (type_identifier) @name body: (_)) @definition.enum

(type_definition
  declarator: [
    (type_identifier) @name
    (pointer_declarator declarator: (type_identifier) @name)
  ]) @definition.type

(preproc_def name: (identifier) @name) @definition.macro

(preproc_function_def name: (identifier) @name) @definition.macro
