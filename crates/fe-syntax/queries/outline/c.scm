;; outline query, ours; capture convention of tree-sitter tags

; A declarator captured as @name stands for the identifier it declares,
; through any chain of pointer, array, function and parenthesized
; declarators: int ***f(void), void (*signal(int, void (*)(int)))(int).
(function_definition declarator: (_) @name) @definition.function

; Only with a body: struct foo *p; isn't a definition.
(struct_specifier name: (type_identifier) @name body: (_)) @definition.struct

(union_specifier name: (type_identifier) @name body: (_)) @definition.struct

(enum_specifier name: (type_identifier) @name body: (_)) @definition.enum

; typedef int A, B; is one item named A, B.
(type_definition
  declarator: (_) @name
  ("," declarator: (_) @name)*) @definition.type

(preproc_def name: (identifier) @name) @definition.macro

(preproc_function_def name: (identifier) @name) @definition.macro
