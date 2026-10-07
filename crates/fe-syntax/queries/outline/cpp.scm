;; outline query, ours, written from tree-sitter-cpp 0.23.4's node-types.json;
;; capture convention of tree-sitter tags
; Standalone rather than outline/c.scm followed by this file: the first
; pattern wins on a node, and C's function pattern would then make every
; member function a Function.
;
; A declarator captured as @name stands for the identifier it declares, as
; in C, and a qualified name for its last part: length in
; double Point::length(). A conversion operator is named up to its
; parameters: operator char*.

; A function defined in a class, struct or union is a method, also in any
; preprocessor branch of its body and as a template. These come before the
; plain function pattern, so they win on the same node.
((function_definition declarator: (_) @name) @definition.method
  (#scope? @definition.method class_specifier struct_specifier union_specifier))

; A member function declared in its class: double length() const;,
; X **clone();, operator bool() const;. Its item is the declarator, as a
; field's is. A pointer to a function, int (*fp)(int);, is a field.
((field_declaration declarator: (_) @name @definition.method)
  (#function? @definition.method))

; Constructors, destructors and conversion operators declared in their
; class, W(int);, ~W(); and operator bool();, which the grammar parses as
; declarations, and member function templates declared there.
((declaration declarator: (_) @name @definition.method) @_declaration
  (#scope? @_declaration class_specifier struct_specifier union_specifier)
  (#function? @definition.method))

; A member defined outside its class: double Point::length() const { },
; X *X::clone(), A::B::X::f(). One whose qualifier names a namespace or a
; namespace alias the file declares, int N::f() { }, is a function: see
; #member? in outline.rs.
((function_definition declarator: (_) @name) @definition.method
  (#member? @definition.method))

(function_definition declarator: (_) @name) @definition.function

(namespace_definition name: (_) @name) @definition.module

(class_specifier name: (type_identifier) @name body: (_)) @definition.class

; Only with a body: struct foo *p; isn't a definition.
(struct_specifier name: (type_identifier) @name body: (_)) @definition.struct

(union_specifier name: (type_identifier) @name body: (_)) @definition.struct

(enum_specifier name: (type_identifier) @name body: (_)) @definition.enum

(type_definition
  declarator: (_) @name
  ("," declarator: (_) @name)*) @definition.type

; using Name = std::string;
(alias_declaration name: (type_identifier) @name) @definition.type

(preproc_def name: (identifier) @name) @definition.macro

(preproc_function_def name: (identifier) @name) @definition.macro
