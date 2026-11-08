; Based on nvim-treesitter's queries at commit
; 19bdfc04f40ed2ccb3340591d125f759b0406875 (Apache-2.0, text in
; LICENSE-APACHE): runtime/queries/hlsl/highlights.scm, which inherits
; runtime/queries/cpp/highlights.scm, which inherits
; runtime/queries/c/highlights.scm. The three are inlined here in that order,
; C first, so HLSL's patterns come last:
; https://github.com/nvim-treesitter/nvim-treesitter/tree/19bdfc04f40ed2ccb3340591d125f759b0406875/runtime/queries
;
; Changes: this header and the section comments; `#lua-match?` became
; `#match?` with the same pattern as a regex; `#set! priority` and `@spell`
; are dropped; patterns using `#has-ancestor?`, `#has-parent?` or their
; `not-` forms are dropped, but for C's field access, kept as a plain
; `(field_expression field: ...)` pattern; C++'s `@constructor` patterns,
; which take every capitalised call for a constructor, are dropped, since
; HLSL has no constructors and its functions are capitalised
; (`GroupMemoryBarrierWithGroupSync()`); HLSL's whole-node attribute pattern
; is replaced, and patterns are added at the end, as the comments there say.
; Last pattern wins.

; ---------------------------------------------------------------- C

(identifier) @variable

(preproc_def
  (preproc_arg) @variable)

[
  "default"
  "goto"
  "asm"
  "__asm__"
] @keyword

[
  "enum"
  "struct"
  "union"
  "typedef"
] @keyword.type

[
  "sizeof"
  "offsetof"
] @keyword.operator

(alignof_expression
  .
  _ @keyword.operator)

"return" @keyword.return

[
  "while"
  "for"
  "do"
  "continue"
  "break"
] @keyword.repeat

[
  "if"
  "else"
  "case"
  "switch"
] @keyword.conditional

[
  "#if"
  "#ifdef"
  "#ifndef"
  "#else"
  "#elif"
  "#endif"
  "#elifdef"
  "#elifndef"
  (preproc_directive)
] @keyword.directive

"#define" @keyword.directive.define

"#include" @keyword.import

[
  ";"
  ":"
  ","
  "."
  "::"
] @punctuation.delimiter

"..." @punctuation.special

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  "="
  "-"
  "*"
  "/"
  "+"
  "%"
  "~"
  "|"
  "&"
  "^"
  "<<"
  ">>"
  "->"
  "<"
  "<="
  ">="
  ">"
  "=="
  "!="
  "!"
  "&&"
  "||"
  "-="
  "+="
  "*="
  "/="
  "%="
  "|="
  "&="
  "^="
  ">>="
  "<<="
  "--"
  "++"
] @operator

; Make sure the comma operator is given a highlight group after the comma
; punctuator so the operator is highlighted properly.
(comma_expression
  "," @operator)

[
  (true)
  (false)
] @boolean

(conditional_expression
  [
    "?"
    ":"
  ] @keyword.conditional.ternary)

(string_literal) @string

(system_lib_string) @string

(escape_sequence) @string.escape

(null) @constant.builtin

(number_literal) @number

(char_literal) @character

(preproc_defined) @function.macro

(field_expression
  field: (field_identifier) @property)

(field_designator) @property

(statement_identifier) @label

(declaration
  type: (type_identifier) @_type
  declarator: (identifier) @label
  (#eq? @_type "__label__"))

[
  (type_identifier)
  (type_descriptor)
] @type

(storage_class_specifier) @keyword.modifier

[
  (type_qualifier)
  (gnu_asm_qualifier)
  "__extension__"
] @keyword.modifier

(linkage_specification
  "extern" @keyword.modifier)

(type_definition
  declarator: (type_identifier) @type.definition)

(primitive_type) @type.builtin

(sized_type_specifier
  _ @type.builtin
  type: _?)

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

(preproc_def
  (preproc_arg) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

(enumerator
  name: (identifier) @constant)

(case_statement
  value: (identifier) @constant)

((identifier) @constant.builtin
  (#any-of? @constant.builtin
    "stderr" "stdin" "stdout"
    "__FILE__" "__LINE__" "__DATE__" "__TIME__"
    "__STDC__" "__STDC_VERSION__" "__STDC_HOSTED__"
    "__cplusplus" "__OBJC__" "__ASSEMBLER__"
    "__BASE_FILE__" "__FILE_NAME__" "__INCLUDE_LEVEL__"
    "__TIMESTAMP__" "__clang__" "__clang_major__"
    "__clang_minor__" "__clang_patchlevel__"
    "__clang_version__" "__clang_literal_encoding__"
    "__clang_wide_literal_encoding__"
    "__FUNCTION__" "__func__" "__PRETTY_FUNCTION__"
    "__VA_ARGS__" "__VA_OPT__"))

(preproc_def
  (preproc_arg) @constant.builtin
  (#any-of? @constant.builtin
    "stderr" "stdin" "stdout"
    "__FILE__" "__LINE__" "__DATE__" "__TIME__"
    "__STDC__" "__STDC_VERSION__" "__STDC_HOSTED__"
    "__cplusplus" "__OBJC__" "__ASSEMBLER__"
    "__BASE_FILE__" "__FILE_NAME__" "__INCLUDE_LEVEL__"
    "__TIMESTAMP__" "__clang__" "__clang_major__"
    "__clang_minor__" "__clang_patchlevel__"
    "__clang_version__" "__clang_literal_encoding__"
    "__clang_wide_literal_encoding__"
    "__FUNCTION__" "__func__" "__PRETTY_FUNCTION__"
    "__VA_ARGS__" "__VA_OPT__"))

(attribute_specifier
  (argument_list
    (identifier) @variable.builtin))

(attribute_specifier
  (argument_list
    (call_expression
      function: (identifier) @variable.builtin)))

((call_expression
  function: (identifier) @function.builtin)
  (#match? @function.builtin "^__builtin_"))

; Preproc def / undef
(preproc_def
  name: (_) @constant.macro)

(preproc_call
  directive: (preproc_directive) @_u
  argument: (_) @constant.macro
  (#eq? @_u "#undef"))

(preproc_ifdef
  name: (identifier) @constant.macro)

(preproc_elifdef
  name: (identifier) @constant.macro)

(preproc_defined
  (identifier) @constant.macro)

(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_expression
    field: (field_identifier) @function.call))

(function_declarator
  declarator: (identifier) @function)

(function_declarator
  declarator: (parenthesized_declarator
    (pointer_declarator
      declarator: (field_identifier) @function)))

(preproc_function_def
  name: (identifier) @function.macro)

(comment) @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^/[*][*][^*](?s:.)*[*]/$"))

; Parameters
(parameter_declaration
  declarator: (identifier) @variable.parameter)

(parameter_declaration
  declarator: (array_declarator) @variable.parameter)

(parameter_declaration
  declarator: (pointer_declarator) @variable.parameter)

(preproc_params
  (identifier) @variable.parameter)

[
  "__attribute__"
  "__declspec"
  "__based"
  "__cdecl"
  "__clrcall"
  "__stdcall"
  "__fastcall"
  "__thiscall"
  "__vectorcall"
  (ms_pointer_modifier)
  (attribute_declaration)
] @attribute

; ---------------------------------------------------------------- C++

((identifier) @variable.member
  (#match? @variable.member "^m_.*$"))

(parameter_declaration
  declarator: (reference_declarator) @variable.parameter)

; function(Foo ...foo)
(variadic_parameter_declaration
  declarator: (variadic_declarator
    (_) @variable.parameter))

; int foo = 0
(optional_parameter_declaration
  declarator: (_) @variable.parameter)

(field_declaration
  (field_identifier) @variable.member)

(field_initializer
  (field_identifier) @property)

(function_declarator
  declarator: (field_identifier) @function.method)

(concept_definition
  name: (identifier) @type.definition)

(alias_declaration
  name: (type_identifier) @type.definition)

(auto) @type.builtin

(namespace_identifier) @module

((namespace_identifier) @type
  (#match? @type "^[A-Z]"))

(case_statement
  value: (qualified_identifier
    (identifier) @constant))

(using_declaration
  .
  "using"
  .
  "namespace"
  .
  [
    (qualified_identifier)
    (identifier)
  ] @module)

(destructor_name
  (identifier) @function.method)

; functions
(function_declarator
  (qualified_identifier
    (identifier) @function))

(function_declarator
  (qualified_identifier
    (qualified_identifier
      (identifier) @function)))

(function_declarator
  (qualified_identifier
    (qualified_identifier
      (qualified_identifier
        (identifier) @function))))

(function_declarator
  (template_function
    (identifier) @function))

(operator_name) @function

"operator" @function

"static_assert" @function.builtin

(call_expression
  (qualified_identifier
    (identifier) @function.call))

(call_expression
  (qualified_identifier
    (qualified_identifier
      (identifier) @function.call)))

(call_expression
  (qualified_identifier
    (qualified_identifier
      (qualified_identifier
        (identifier) @function.call))))

(call_expression
  (template_function
    (identifier) @function.call))

(call_expression
  (qualified_identifier
    (template_function
      (identifier) @function.call)))

(call_expression
  (qualified_identifier
    (qualified_identifier
      (template_function
        (identifier) @function.call))))

(call_expression
  (qualified_identifier
    (qualified_identifier
      (qualified_identifier
        (template_function
          (identifier) @function.call)))))

; methods
(function_declarator
  (template_method
    (field_identifier) @function.method))

(call_expression
  (field_expression
    (field_identifier) @function.method.call))

(call_expression
  (field_expression
    (template_method
      (field_identifier) @function.method.call)))

; Constants
(this) @variable.builtin

(null
  "nullptr" @constant.builtin)

(true) @boolean

(false) @boolean

; Literals
(raw_string_literal) @string

; Keywords
[
  "try"
  "catch"
  "noexcept"
  "throw"
] @keyword.exception

[
  "decltype"
  "explicit"
  "friend"
  "override"
  "using"
  "requires"
  "constexpr"
] @keyword

[
  "class"
  "namespace"
  "template"
  "typename"
  "concept"
] @keyword.type

[
  "co_await"
  "co_yield"
  "co_return"
] @keyword.coroutine

[
  "public"
  "private"
  "protected"
  "final"
  "virtual"
] @keyword.modifier

[
  "new"
  "delete"
  "xor"
  "bitand"
  "bitor"
  "compl"
  "not"
  "xor_eq"
  "and_eq"
  "or_eq"
  "not_eq"
  "and"
  "or"
] @keyword.operator

"<=>" @operator

"::" @punctuation.delimiter

(template_argument_list
  [
    "<"
    ">"
  ] @punctuation.bracket)

(template_parameter_list
  [
    "<"
    ">"
  ] @punctuation.bracket)

(literal_suffix) @operator

; ---------------------------------------------------------------- HLSL

[
  "in"
  "out"
  "inout"
  "uniform"
  "shared"
  "groupshared"
  "discard"
  "cbuffer"
  "row_major"
  "column_major"
  "globallycoherent"
  "centroid"
  "noperspective"
  "nointerpolation"
  "sample"
  "linear"
  "snorm"
  "unorm"
  "point"
  "line"
  "triangleadj"
  "lineadj"
  "triangle"
] @keyword.modifier

((identifier) @variable.builtin
  (#match? @variable.builtin "^SV_"))

(hlsl_attribute
  [
    "["
    "]"
  ] @attribute)

; Ours, from here to the end.

; An attribute's name, `[unroll]` or `[numthreads(8, 8, 1)]`, in place of
; upstream's `(hlsl_attribute) @attribute`, which also paints the spaces
; between its arguments.
(hlsl_attribute
  (identifier) @attribute)

(hlsl_attribute
  (call_expression
    function: (identifier) @attribute))

; The scalar, vector and matrix types, which this grammar parses as type
; names (`float3`), or as calls where they convert (`float4(v, 1.0)`).
((type_identifier) @type.builtin
  (#match? @type.builtin "^(bool|int|uint|dword|half|float|double|min16float|min10float|min16int|min12int|min16uint|u?int(16|32|64)_t|float(16|32|64)_t)([1-4](x[1-4])?)?$"))

((type_identifier) @type.builtin
  (#any-of? @type.builtin "vector" "matrix"))

((call_expression
  function: (identifier) @type.builtin)
  (#match? @type.builtin "^(bool|int|uint|dword|half|float|double|min16float|min10float|min16int|min12int|min16uint|u?int(16|32|64)_t|float(16|32|64)_t)([1-4](x[1-4])?)?$"))

; A macro whose value is a number: `#define MAX_LIGHTS 4`.
((preproc_arg) @number
  (#match? @number "^[ \t]*[-+]?(0[xX][0-9a-fA-F]+|[0-9]+[.]?[0-9]*([eE][-+]?[0-9]+)?|[.][0-9]+([eE][-+]?[0-9]+)?)[uUlLfFhH]*[ \t]*$"))

; `precise`, which this grammar parses inside `(qualifiers)`.
"precise" @keyword.modifier

; This grammar parses `cbuffer Name { ... }` as a function definition whose
; return type is `cbuffer`.
((function_definition
  type: (type_identifier) @keyword
  declarator: (identifier) @type)
  (#any-of? @keyword "cbuffer" "tbuffer"))
