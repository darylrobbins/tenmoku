;; outline query, ours; capture convention of tree-sitter tags

(class_declaration name: (identifier) @name) @definition.class

(record_declaration name: (identifier) @name) @definition.class

(interface_declaration name: (identifier) @name) @definition.interface

(annotation_type_declaration name: (identifier) @name) @definition.interface

(enum_declaration name: (identifier) @name) @definition.enum

(method_declaration name: (identifier) @name) @definition.method

(constructor_declaration name: (identifier) @name) @definition.method

; A record's compact constructor: record R(int x) { R { ... } }.
(compact_constructor_declaration name: (identifier) @name) @definition.method

; An annotation type's elements: @interface Marker { String value(); }.
(annotation_type_element_declaration name: (identifier) @name) @definition.method
