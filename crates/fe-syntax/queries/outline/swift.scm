;; outline query, ours, written from tree-sitter-swift 0.7.3's
;; node-types.json; capture convention of tree-sitter tags

; The grammar gives a function's return type its name field too, so a
; function's name is the identifier or operator there: func ==(...).
; Functions, initializers, deinitializers and subscripts in the body of a
; type, an extension or a protocol are methods. These come before the
; plain function pattern, so they win on the same node.
[
  (class_body
    [
      (function_declaration
        name: [
          (simple_identifier)
          (custom_operator)
          "!=" "!==" "%" "%=" "&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "/" "/="
          "<" "<<" "<=" "=" "==" "===" ">" ">=" ">>" "^" (bang) "|" "~"
        ] @name)
      (init_declaration name: "init" @name)
      (deinit_declaration "deinit" @name)
      (subscript_declaration "subscript" @name)
    ] @definition.method)
  (enum_class_body
    [
      (function_declaration
        name: [
          (simple_identifier)
          (custom_operator)
          "!=" "!==" "%" "%=" "&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "/" "/="
          "<" "<<" "<=" "=" "==" "===" ">" ">=" ">>" "^" (bang) "|" "~"
        ] @name)
      (init_declaration name: "init" @name)
      (deinit_declaration "deinit" @name)
      (subscript_declaration "subscript" @name)
    ] @definition.method)
]

(protocol_body
  [
    (protocol_function_declaration
      name: [
        (simple_identifier)
        (custom_operator)
        "!=" "!==" "%" "%=" "&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "/" "/="
        "<" "<<" "<=" "=" "==" "===" ">" ">=" ">>" "^" (bang) "|" "~"
      ] @name)
    (init_declaration name: "init" @name)
    (subscript_declaration "subscript" @name)
  ] @definition.method)

(function_declaration
  name: [
    (simple_identifier)
    (custom_operator)
    "!=" "!==" "%" "%=" "&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "/" "/="
    "<" "<<" "<=" "=" "==" "===" ">" ">=" ">>" "^" (bang) "|" "~"
  ] @name) @definition.function

; One node kind for classes, actors, structs, enums and extensions.
(class_declaration
  declaration_kind: ["class" "actor"]
  name: (type_identifier) @name) @definition.class

(class_declaration
  declaration_kind: "struct"
  name: (type_identifier) @name) @definition.struct

(class_declaration
  declaration_kind: "enum"
  name: (type_identifier) @name) @definition.enum

; extension Point, named by the type it extends.
(class_declaration
  declaration_kind: "extension"
  name: (_) @name) @definition.impl

(protocol_declaration name: (type_identifier) @name) @definition.interface

(typealias_declaration name: (type_identifier) @name) @definition.type

; macro stringify<T>(_ value: T) -> (T, String) = #externalMacro(...).
(macro_declaration (simple_identifier) @name) @definition.macro

; A protocol's associatedtype Item.
(associatedtype_declaration name: (type_identifier) @name) @definition.type
