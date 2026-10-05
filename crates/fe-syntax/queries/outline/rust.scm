;; outline query, ours; capture convention of tree-sitter tags

; A fn in an impl or trait is a method. These come before the plain fn
; pattern, so they win on the same node.
(impl_item
  body: (declaration_list
    (function_item name: (identifier) @name) @definition.method))

(trait_item
  body: (declaration_list
    [
      (function_item name: (identifier) @name)
      (function_signature_item name: (identifier) @name)
    ] @definition.method))

(function_item name: (identifier) @name) @definition.function

(mod_item name: (identifier) @name) @definition.module

(struct_item name: (type_identifier) @name) @definition.struct

(union_item name: (type_identifier) @name) @definition.struct

(enum_item name: (type_identifier) @name) @definition.enum

(trait_item name: (type_identifier) @name) @definition.trait

; Named by its header, from "impl" to the type: impl<T> Display for Foo<T>.
(impl_item "impl" @name type: (_) @name) @definition.impl

(type_item name: (type_identifier) @name) @definition.type

(const_item name: (identifier) @name) @definition.constant

(static_item name: (identifier) @name) @definition.constant

(macro_definition name: (identifier) @name) @definition.macro
