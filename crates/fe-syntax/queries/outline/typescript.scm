;; outline query, ours; capture convention of tree-sitter tags
; Follows javascript.scm.

(interface_declaration name: (_) @name) @definition.interface

(type_alias_declaration name: (_) @name) @definition.type

(enum_declaration name: (_) @name) @definition.enum

(abstract_class_declaration name: (_) @name) @definition.class

(internal_module name: (_) @name) @definition.module

(module name: (_) @name) @definition.module

(method_signature name: (_) @name) @definition.method

(abstract_method_signature name: (_) @name) @definition.method

(function_signature name: (_) @name) @definition.function
