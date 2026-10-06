;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/c.scm in the symbols query.

; A declarator captured as @name stands for the identifier it declares, as
; in the outline query. Each declarator of int a, *b; is its own item. Only
; declarators count: the grammar also puts calling conventions, as in
; int __stdcall h(void);, and assembly labels, as in
; int real __asm__("real");, in the declarator field.

; Fields of a named struct or union, also in any preprocessor branch of its
; body, however nested. An anonymous one's aren't listed: #scope? sees
; through the conditionals and rejects a struct or union without a name.
((field_declaration
  declarator: [
    (field_identifier)
    (pointer_declarator)
    (array_declarator)
    (function_declarator)
    (parenthesized_declarator)
    (attributed_declarator)
  ] @name @definition.field) @_declaration
  (#scope? @_declaration struct_specifier union_specifier))

(enumerator name: (identifier) @name) @definition.variant

; A file-scope declaration, also in any branch of an #if, #ifdef, #elif or
; #else at the top, however nested, such as an include guard. A prototype,
; int f(void);, is left out: one whose declarator declares a function isn't
; listed as a variable.
((declaration
  declarator: [
    (identifier)
    (init_declarator)
    (pointer_declarator)
    (array_declarator)
    (function_declarator)
    (parenthesized_declarator)
    (attributed_declarator)
  ] @name @definition.variable) @_declaration
  (#scope? @_declaration translation_unit))
