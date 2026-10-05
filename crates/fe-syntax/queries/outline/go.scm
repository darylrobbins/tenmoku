;; outline query, ours; capture convention of tree-sitter tags

(function_declaration name: (identifier) @name) @definition.function

; Named without its receiver.
(method_declaration name: (field_identifier) @name) @definition.method

; Struct and interface types come before the plain type pattern, so they win
; on the same node.
(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct

(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface

(type_spec name: (type_identifier) @name) @definition.type

(type_alias name: (type_identifier) @name) @definition.type

(method_elem name: (field_identifier) @name) @definition.method

(source_file
  (const_declaration
    (const_spec name: (identifier) @name) @definition.constant))
