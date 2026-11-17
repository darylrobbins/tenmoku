; Adapted from victorhqc/tree-sitter-prisma's queries/highlights.scm at
; commit 3556b2c1f20ec9ac91e92d32c43d9d2a0ca3cc49 (MIT, the grammar's own
; licence and holder, text in LICENSE):
; https://github.com/victorhqc/tree-sitter-prisma/blob/3556b2c1f20ec9ac91e92d32c43d9d2a0ca3cc49/queries/highlights.scm
;
; Changes: this header; "type" (composite types) joins the keywords; a `///`
; comment, which the grammar calls `comment` (a `//` one is
; `developer_comment`), is @comment.documentation; the pattern that captured
; a whole argument list as @property is dropped, so arguments keep their own
; colours, and a named argument's name (`fields:`) is @variable.parameter;
; an enum value is @constant.enum_member rather than its whole line
; @constant; attributes are @attribute rather than @label, and the member of
; a dotted one (`@db.VarChar`) is coloured too; model, view, type and enum
; names are @type, the scalar types @type.builtin, field names and a block's
; setting names (`provider =`) @property; null is @constant.builtin; "," ":"
; and "." are @punctuation.delimiter and `?` @punctuation.special. Last
; pattern wins.

(identifier) @variable

[
  "datasource"
  "enum"
  "generator"
  "model"
  "type"
  "view"
] @keyword

(developer_comment) @comment

(comment) @comment.documentation

(number) @number

(string) @string

[
  (true)
  (false)
] @boolean

(null) @constant.builtin

; Declarations
; ------------

(model_declaration
  (identifier) @type)

(view_declaration
  (identifier) @type)

(type_declaration
  (identifier) @type)

(type_declaration_type) @type

(enum_declaration
  (identifier) @type)

(enumeral
  (identifier) @constant.enum_member)

; A setting of a datasource or generator block.
(assignment_expression
  (variable) @property)

; Fields
; ------

(column_declaration
  (identifier) @property)

(column_type
  (identifier) @type)

(column_type
  (call_expression
    (identifier) @type))

((column_type
  [
    (identifier) @type.builtin
    (call_expression
      (identifier) @type.builtin)
  ])
  (#any-of? @type.builtin
    "BigInt" "Boolean" "Bytes" "DateTime" "Decimal" "Float" "Int" "Json"
    "String" "Unsupported"))

(maybe) @punctuation.special

; Calls and arguments
; -------------------

(member_expression
  (property_identifier) @property)

(call_expression
  (identifier) @function)

(type_expression
  .
  (identifier) @variable.parameter)

; Attributes
; ----------

[
  "@"
  "@@"
] @attribute

(attribute
  (identifier) @attribute)

(attribute
  (call_expression
    (identifier) @attribute))

(attribute
  (member_expression
    [
      (identifier)
      (property_identifier)
    ] @attribute))

(attribute
  (call_expression
    (member_expression
      [
        (identifier)
        (property_identifier)
      ] @attribute)))

(block_attribute_declaration
  (identifier) @attribute)

(block_attribute_declaration
  (call_expression
    (identifier) @attribute))

(block_attribute_declaration
  (call_expression
    (member_expression
      [
        (identifier)
        (property_identifier)
      ] @attribute)))

; Punctuation
; -----------

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ":"
  "."
] @punctuation.delimiter

"=" @operator
