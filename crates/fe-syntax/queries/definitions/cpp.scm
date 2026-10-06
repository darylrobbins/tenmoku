;; definitions query, ours, written from tree-sitter-cpp 0.23.4's
;; node-types.json; capture convention of tree-sitter tags
; Follows outline/cpp.scm in the symbols query.
;
; Only declarators count, as in C: the grammar also puts calling
; conventions and assembly labels in the declarator field. Each declarator
; of int a, *b; is its own item.

; Data members of a named class, struct or union, also in any preprocessor
; branch of its body. A member function's declarator is a method in the
; outline query, which wins on that node; a pointer to a function,
; int (*fp)(int);, is a field.
((field_declaration
  declarator: [
    (field_identifier)
    (pointer_declarator)
    (array_declarator)
    (reference_declarator)
    (function_declarator)
    (parenthesized_declarator)
    (attributed_declarator)
  ] @name @definition.field) @_declaration
  (#scope? @_declaration class_specifier struct_specifier union_specifier))

; A static data member template: template <class T> static T zero;.
((template_declaration
  (declaration
    declarator: [
      (identifier)
      (init_declarator)
      (pointer_declarator)
      (array_declarator)
      (reference_declarator)
      (parenthesized_declarator)
      (attributed_declarator)
    ] @name @definition.field)) @_template
  (#scope? @_template class_specifier struct_specifier union_specifier)
  (#not-function? @definition.field))

; A static data member defined outside its class, int X::count = 0;, at
; file scope or in a namespace, also as a template. Named count; one whose
; qualifier is a namespace, int N::x = 0;, is a variable below.
((declaration
  declarator: [
    (qualified_identifier)
    (init_declarator)
    (pointer_declarator)
    (array_declarator)
    (reference_declarator)
    (parenthesized_declarator)
    (attributed_declarator)
  ] @name @definition.field) @_declaration
  (#scope? @_declaration translation_unit declaration_list)
  (#member? @definition.field)
  (#not-function? @definition.field))

((template_declaration
  (declaration
    declarator: [
      (qualified_identifier)
      (init_declarator)
      (pointer_declarator)
      (array_declarator)
      (reference_declarator)
      (parenthesized_declarator)
      (attributed_declarator)
    ] @name @definition.field)) @_template
  (#scope? @_template translation_unit declaration_list)
  (#member? @definition.field)
  (#not-function? @definition.field))

(enumerator name: (identifier) @name) @definition.variant

; A declaration at file scope or directly in a namespace or an extern "C"
; block, whose bodies are declaration lists, also in any branch of an #if
; there, and a variable template, template <class T> T pi = T(3);. A
; prototype, int f(int);, isn't listed, as in C.
((declaration
  declarator: [
    (identifier)
    (qualified_identifier)
    (init_declarator)
    (pointer_declarator)
    (array_declarator)
    (reference_declarator)
    (function_declarator)
    (parenthesized_declarator)
    (attributed_declarator)
  ] @name @definition.variable) @_declaration
  (#scope? @_declaration translation_unit declaration_list)
  (#not-member? @definition.variable))

((template_declaration
  (declaration
    declarator: [
      (identifier)
      (qualified_identifier)
      (init_declarator)
      (pointer_declarator)
      (array_declarator)
      (reference_declarator)
      (parenthesized_declarator)
      (attributed_declarator)
    ] @name @definition.variable)) @_template
  (#scope? @_template translation_unit declaration_list)
  (#not-member? @definition.variable))
