;; outline query, adapted from tree-sitter-php 0.24.2 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Changed: only @definition.* and @name are kept (no references); a method
; is a method rather than a function and a trait a trait rather than an
; interface; enums, constants and define() are added.

(namespace_definition name: (namespace_name) @name) @definition.module

(interface_declaration name: (name) @name) @definition.interface

(trait_declaration name: (name) @name) @definition.trait

(class_declaration name: (name) @name) @definition.class

(enum_declaration name: (name) @name) @definition.enum

(function_definition name: (name) @name) @definition.function

(method_declaration name: (name) @name) @definition.method

; const APP_VERSION = '1.0'; at the top or in a class, each element its own
; item.
(const_element (name) @name) @definition.constant

; define('LEGACY', true);, named by the string's content.
((function_call_expression
  function: (name) @_define
  arguments: (arguments
    .
    (argument (string (string_content) @name)))) @definition.constant
  (#eq? @_define "define"))
