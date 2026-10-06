;; outline query, adapted from tree-sitter-kotlin-sg 0.4.1 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Changed: only @definition.* and @name are kept (no references); a
; function in a class, object or enum body is a method, as a secondary
; constructor is; interfaces and enums have their own kinds; a const val
; is a constant, and other properties move to definitions/kotlin.scm; the
; package header is a module.
; The grammar has no fields: a declaration's name is its child of the
; name's node kind.

[
  (class_body
    [
      (function_declaration (simple_identifier) @name)
      (secondary_constructor "constructor" @name)
    ] @definition.method)
  (enum_class_body
    [
      (function_declaration (simple_identifier) @name)
      (secondary_constructor "constructor" @name)
    ] @definition.method)
]

(function_declaration (simple_identifier) @name) @definition.function

; package demo.shapes, which lists only the header: the grammar doesn't make
; the rest of the file its body.
(package_header (identifier) @name) @definition.module

(class_declaration "interface" (type_identifier) @name) @definition.interface

(class_declaration "enum" (type_identifier) @name) @definition.enum

; A data class or any other class.
(class_declaration "class" (type_identifier) @name) @definition.class

(object_declaration (type_identifier) @name) @definition.class

; Only a named companion object: an unnamed one's members stay under the
; class.
(companion_object (type_identifier) @name) @definition.class

(type_alias (type_identifier) @name) @definition.type

; const val LIMIT = 10. It comes before the definitions query's property
; patterns, so it wins on the same node.
(property_declaration
  (modifiers (property_modifier "const"))
  (variable_declaration (simple_identifier) @name)) @definition.constant
