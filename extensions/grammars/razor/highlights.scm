; Razor highlights. From tree-sitter-razor at
; d4664e409caaea12f73c9525484e3cf88b1cf718,
; https://github.com/tris203/tree-sitter-razor, path queries/highlights.scm,
; license MIT. Upstream's query starts `; inherits: c_sharp`: the grammar
; extends tree-sitter-c-sharp ^0.23.5 and parses the C# itself, so this file
; first inlines tree-sitter-c-sharp 0.23.5's queries/highlights.scm (MIT,
; https://github.com/tree-sitter/tree-sitter-c-sharp at
; cac6d5fb595f5811a076336682d5d595ac1c9e85; its licence, with its copyright
; notice, is in LICENSE-TREE-SITTER-CSHARP), unchanged and the same as
; extensions/grammars/csharp/highlights.scm, and then Razor's patterns, so
; that with the last pattern winning (`earlier_patterns_win = false`, as
; csharp/ has) Razor's win where both match. Changes to Razor's
; patterns: directives (`@page`, `@code`, `@@`, `@:` ...) are
; `@keyword.directive` rather than Neovim's `@constant.macro`, which
; fe-syntax would colour as a constant; the `@` of `@expr` and `@(expr)` is
; `@punctuation.special` rather than `@variable`; `@keyword.coroutine`,
; `@keyword.conditional`, `@keyword.repeat` and `@keyword.exception` are
; kept, and fe-syntax colours them as `@keyword`. Added after them: the
; type `@model`, `@inherits`, `@implements` and `@layout` name, and the `=`
; of an element's attributes as punctuation. The markup inside each
; `(element)` is HTML, injected (injections.scm), which colours its tags and
; attributes.

; ---- tree-sitter-c-sharp 0.23.5, queries/highlights.scm ----

(identifier) @variable

;; Methods

(method_declaration name: (identifier) @function)
(local_function_statement name: (identifier) @function)

;; Types

(interface_declaration name: (identifier) @type)
(class_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(struct_declaration (identifier) @type)
(record_declaration (identifier) @type)
(namespace_declaration name: (identifier) @module)

(generic_name (identifier) @type)
(type_parameter (identifier) @property.definition)
(parameter type: (identifier) @type)
(type_argument_list (identifier) @type)
(as_expression right: (identifier) @type)
(is_expression right: (identifier) @type)

(constructor_declaration name: (identifier) @constructor)
(destructor_declaration name: (identifier) @constructor)

(_ type: (identifier) @type)

(base_list (identifier) @type)

(predefined_type) @type.builtin

;; Enum
(enum_member_declaration (identifier) @property.definition)

;; Literals

[
  (real_literal)
  (integer_literal)
] @number

[
  (character_literal)
  (string_literal)
  (raw_string_literal)
  (verbatim_string_literal)
  (interpolated_string_expression)
  (interpolation_start)
  (interpolation_quote)
 ] @string

(escape_sequence) @string.escape

[
  (boolean_literal)
  (null_literal)
] @constant.builtin

;; Comments

(comment) @comment

;; Tokens

[
  ";"
  "."
  ","
] @punctuation.delimiter

[
  "--"
  "-"
  "-="
  "&"
  "&="
  "&&"
  "+"
  "++"
  "+="
  "<"
  "<="
  "<<"
  "<<="
  "="
  "=="
  "!"
  "!="
  "=>"
  ">"
  ">="
  ">>"
  ">>="
  ">>>"
  ">>>="
  "|"
  "|="
  "||"
  "?"
  "??"
  "??="
  "^"
  "^="
  "~"
  "*"
  "*="
  "/"
  "/="
  "%"
  "%="
  ":"
  ".."
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
  (interpolation_brace)
]  @punctuation.bracket

;; Keywords

[
  (modifier)
  "this"
  (implicit_type)
] @keyword

[
  "add"
  "alias"
  "as"
  "base"
  "break"
  "case"
  "catch"
  "checked"
  "class"
  "continue"
  "default"
  "delegate"
  "do"
  "else"
  "enum"
  "event"
  "explicit"
  "extern"
  "finally"
  "for"
  "foreach"
  "global"
  "goto"
  "if"
  "implicit"
  "interface"
  "is"
  "lock"
  "namespace"
  "notnull"
  "operator"
  "params"
  "return"
  "remove"
  "sizeof"
  "stackalloc"
  "static"
  "struct"
  "switch"
  "throw"
  "try"
  "typeof"
  "unchecked"
  "using"
  "while"
  "new"
  "await"
  "in"
  "yield"
  "get"
  "set"
  "when"
  "out"
  "ref"
  "from"
  "where"
  "select"
  "record"
  "init"
  "with"
  "let"
] @keyword

;; Attribute

(attribute name: (identifier) @attribute)

;; Parameters

(parameter
  name: (identifier) @variable.parameter)

;; Type constraints

(type_parameter_constraints_clause (identifier) @property.definition)

;; Method calls

(invocation_expression (member_access_expression name: (identifier) @function))

; ---- tree-sitter-razor, queries/highlights.scm ----

[
  (razor_comment)
  (html_comment)
] @comment

[
  "at_page"
  "at_using"
  "at_model"
  "at_rendermode"
  "at_inject"
  "at_implements"
  "at_layout"
  "at_inherits"
  "at_attribute"
  "at_typeparam"
  "at_namespace"
  "at_preservewhitespace"
  "at_block"
  "at_at_escape"
  "at_colon_transition"
  "at_addtaghelper"
  "at_removetaghelper"
  "at_taghelperprefix"
] @keyword.directive

[
  "at_lock"
  "at_section"
] @keyword

[
  "at_if"
  "at_switch"
] @keyword.conditional

[
  "at_for"
  "at_foreach"
  "at_while"
  "at_do"
] @keyword.repeat

[
  "at_try"
  "catch"
  "finally"
] @keyword.exception

[
  "at_implicit"
  "at_explicit"
] @punctuation.special

(razor_implicit_expression
  "at_implicit" @keyword.coroutine
  (await_expression
    "await" @keyword.coroutine))

(razor_rendermode) @property

(razor_attribute_name) @function

(taghelper_wildcard) @character.special

; ---- Added here ----

; The type a directive names: `@model Shop.Pages.OrderModel`,
; `@inherits BasePage`, `@implements IDisposable`, `@layout MainLayout`.
[
  (razor_model_directive name: (identifier) @type)
  (razor_model_directive name: (qualified_name name: (identifier) @type))
  (razor_inherits_directive name: (identifier) @type)
  (razor_inherits_directive name: (qualified_name name: (identifier) @type))
  (razor_implements_directive name: (identifier) @type)
  (razor_implements_directive name: (qualified_name name: (identifier) @type))
  (razor_layout_directive name: (identifier) @type)
  (razor_layout_directive name: (qualified_name name: (identifier) @type))
]

; An HTML attribute's `=` is the element's own token, which the injected
; HTML leaves uncaptured: punctuation, rather than C#'s operator.
(element "=" @punctuation.delimiter)
