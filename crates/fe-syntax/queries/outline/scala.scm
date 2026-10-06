;; outline query, adapted from tree-sitter-scala 0.26.2 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Changed: only @definition.* and @name are kept (no references); an object
; is a module and a trait a trait; a def in a class, trait, object or enum
; body is a method; enum cases, vals, vars and class parameters move to
; definitions/scala.scm.

; A package's braces are a template body too: package p { def f = 1 }. Its
; defs are functions, and this comes first, so it wins on the same node.
(package_clause
  body: (template_body
    [
      (function_definition name: (_) @name)
      (function_declaration name: (_) @name)
    ] @definition.function))

[
  (template_body
    [
      (function_definition name: (_) @name)
      (function_declaration name: (_) @name)
    ] @definition.method)
  (enum_body
    [
      (function_definition name: (_) @name)
      (function_declaration name: (_) @name)
    ] @definition.method)
]

(function_definition name: (_) @name) @definition.function

(function_declaration name: (_) @name) @definition.function

(package_clause name: (package_identifier) @name) @definition.module

(object_definition name: (identifier) @name) @definition.module

(trait_definition name: (identifier) @name) @definition.trait

(class_definition name: (identifier) @name) @definition.class

(enum_definition name: (identifier) @name) @definition.enum

(type_definition name: (type_identifier) @name) @definition.type
