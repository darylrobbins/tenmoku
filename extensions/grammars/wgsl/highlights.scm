; Adapted from szebniok/tree-sitter-wgsl's queries/highlights.scm at commit
; 40259f3c77ea856841a4e0c4c807705f3e4a2b65 (MIT, see LICENSE). Captures renamed
; to fe-syntax's vocabulary; the (ERROR) pattern dropped.

(int_literal) @number
(float_literal) @number
(bool_literal) @constant.builtin

; A call such as max(a, b) parses with a bare-identifier type_declaration; it
; goes before the generic type pattern, which would win. Constructors with
; template arguments (vec3<f32>(..)) keep the type colours.
(type_constructor_or_function_call_expression
    (type_declaration . (identifier) @function .))

(type_declaration [ "bool" "u32" "i32" "f16" "f32" ] @type.builtin)
(type_declaration) @type

(function_declaration
    (identifier) @function)

(parameter
    (variable_identifier_declaration (identifier) @variable.parameter))

(struct_declaration
    (identifier) @type)

(struct_declaration
    (struct_member (variable_identifier_declaration (identifier) @property)))

(attribute
    (identifier) @attribute)

(identifier) @variable

[
    "struct"
    "bitcast"
    "discard"
    "enable"
    "fallthrough"
    "let"
    "type"
    "var"
    "override"
    (texel_format)
    "fn"
    "return"
    "private"
    "storage"
    "uniform"
    "workgroup"
    "read"
    "read_write"
    "write"
    "loop"
    "for"
    "while"
    "break"
    "continue"
    "continuing"
    "if"
    "else"
    "switch"
    "case"
    "default"
] @keyword

(compound_assignment_operator) @operator

[ "," "." ":" ";" "->" ] @punctuation.delimiter

["(" ")" "[" "]" "{" "}"] @punctuation.bracket

[
    "&"
    "&&"
    "/"
    "!"
    "="
    "=="
    "!="
    ">"
    ">="
    ">>"
    "<"
    "<="
    "<<"
    "%"
    "-"
    "+"
    "|"
    "||"
    "*"
    "~"
    "^"
    "@"
    "++"
    "--"
] @operator

[
    (line_comment)
    (block_comment)
] @comment
