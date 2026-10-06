;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/c.scm in the symbols query.

; A declarator captured as @name stands for the identifier it declares, as
; in the outline query. Each declarator of int a, *b; is its own item.

; Fields of a named struct or union. An anonymous one's aren't listed.
(struct_specifier
  name: (_)
  body: (field_declaration_list
    (field_declaration declarator: (_) @name @definition.field)))

(union_specifier
  name: (_)
  body: (field_declaration_list
    (field_declaration declarator: (_) @name @definition.field)))

(enumerator name: (identifier) @name) @definition.variant

; A file-scope declaration, also inside an include guard or another #if at
; the top. A prototype, int f(void);, is left out: one whose declarator
; declares a function isn't listed as a variable.
(translation_unit
  (declaration declarator: (_) @name @definition.variable))

(translation_unit
  [
    (preproc_if
      (declaration declarator: (_) @name @definition.variable))
    (preproc_ifdef
      (declaration declarator: (_) @name @definition.variable))
  ])
