; Objective-C highlights, later patterns winning (earlier_patterns_win = false).
;
; The C part is queries/highlights.scm of tree-sitter-c 0.24.2 (the crate
; Cargo.lock pins), verbatim: upstream's query starts with `; inherits: c`.
; Its MIT licence and copyright are in LICENSE-TREE-SITTER-C.
;
; The Objective-C part is adapted from queries/highlights.scm of
; tree-sitter-grammars/tree-sitter-objc at
; 181a81b8f23a2d593e7ab4259981f50122909fda, under the grammar's MIT licence
; (LICENSE). Adapted: the `#has-ancestor?` pattern (fe-syntax has no such
; predicate) rewritten as the struct declarators it reaches, `@text.uri`
; dropped from version numbers, `<` and `>` coloured as brackets only where
; they are (upstream colours every less-than), and the patterns marked "Ours"
; added.

; C
; --------------------

(identifier) @variable

((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\\d_]*$"))

"break" @keyword
"case" @keyword
"const" @keyword
"continue" @keyword
"default" @keyword
"do" @keyword
"else" @keyword
"enum" @keyword
"extern" @keyword
"for" @keyword
"if" @keyword
"inline" @keyword
"return" @keyword
"sizeof" @keyword
"static" @keyword
"struct" @keyword
"switch" @keyword
"typedef" @keyword
"union" @keyword
"volatile" @keyword
"while" @keyword

"#define" @keyword
"#elif" @keyword
"#else" @keyword
"#endif" @keyword
"#if" @keyword
"#ifdef" @keyword
"#ifndef" @keyword
"#include" @keyword
(preproc_directive) @keyword

"--" @operator
"-" @operator
"-=" @operator
"->" @operator
"=" @operator
"!=" @operator
"*" @operator
"&" @operator
"&&" @operator
"+" @operator
"++" @operator
"+=" @operator
"<" @operator
"==" @operator
">" @operator
"||" @operator

"." @delimiter
";" @delimiter

(string_literal) @string
(system_lib_string) @string

(null) @constant
(number_literal) @number
(char_literal) @number

(field_identifier) @property
(statement_identifier) @label
(type_identifier) @type
(primitive_type) @type
(sized_type_specifier) @type

(call_expression
  function: (identifier) @function)
(call_expression
  function: (field_expression
    field: (field_identifier) @function))
(function_declarator
  declarator: (identifier) @function)
(preproc_function_def
  name: (identifier) @function.special)

(comment) @comment

; Objective-C
; --------------------

; Ours: C keywords and literals tree-sitter-c's query leaves plain.

[
  "goto"
  "#elifdef"
  "#elifndef"
  "#undef"
  (storage_class_specifier)
  (type_qualifier)
] @keyword

(escape_sequence) @escape

[
  (true)
  (false)
] @constant.builtin

((identifier) @constant.builtin
  (#any-of? @constant.builtin "nil" "Nil" "YES" "NO"))

; Preprocs

(preproc_undef
  name: (_) @constant) @preproc

; Includes

(module_import "@import" @include path: (identifier) @namespace)

((preproc_include
  _ @include path: (_))
  (#any-of? @include "#include" "#import"))

; Type Qualifiers

[
  "@optional"
  "@required"
  "__covariant"
  "__contravariant"
  (visibility_specification)
] @type.qualifier

; Storageclasses

[
  "@autoreleasepool"
  "@synthesize"
  "@dynamic"
  "volatile"
  (protocol_qualifier)
] @storageclass

; Keywords

[
  "@protocol"
  "@interface"
  "@implementation"
  "@compatibility_alias"
  "@property"
  "@selector"
  "@defs"
  "availability"
  "@end"
] @keyword

(class_declaration "@" @keyword "class" @keyword)

(method_definition ["+" "-"] @keyword.function)
(method_declaration ["+" "-"] @keyword.function)

[
  "__typeof__"
  "__typeof"
  "typeof"
  "in"
] @keyword.operator

[
  "@synchronized"
  "oneway"
] @keyword.coroutine

; Exceptions

[
  "@try"
  "__try"
  "@catch"
  "__catch"
  "@finally"
  "__finally"
  "@throw"
] @exception

; Variables

((identifier) @variable.builtin
  (#any-of? @variable.builtin "self" "super"))

; Functions & Methods

[
  "objc_bridge_related"
  "@available"
  "__builtin_available"
  "va_arg"
  "asm"
] @function.builtin

(method_definition (identifier) @method)

(method_declaration (identifier) @method)

(method_identifier (identifier)? @method ":" @method (identifier)? @method)

(message_expression method: (identifier) @method.call)

; Ours: the method a selector names.

(selector_expression (identifier) @method)

; Constructors

((message_expression method: (identifier) @constructor)
  (#eq? @constructor "init"))

; Attributes

(availability_attribute_specifier
  [
    "CF_FORMAT_FUNCTION" "NS_AVAILABLE" "__IOS_AVAILABLE" "NS_AVAILABLE_IOS"
    "API_AVAILABLE" "API_UNAVAILABLE" "API_DEPRECATED" "NS_ENUM_AVAILABLE_IOS"
    "NS_DEPRECATED_IOS" "NS_ENUM_DEPRECATED_IOS" "NS_FORMAT_FUNCTION" "DEPRECATED_MSG_ATTRIBUTE"
    "__deprecated_msg" "__deprecated_enum_msg" "NS_SWIFT_NAME" "NS_SWIFT_UNAVAILABLE"
    "NS_EXTENSION_UNAVAILABLE_IOS" "NS_CLASS_AVAILABLE_IOS" "NS_CLASS_DEPRECATED_IOS" "__OSX_AVAILABLE_STARTING"
    "NS_ROOT_CLASS" "NS_UNAVAILABLE" "NS_REQUIRES_NIL_TERMINATION" "CF_RETURNS_RETAINED"
    "CF_RETURNS_NOT_RETAINED" "DEPRECATED_ATTRIBUTE" "UI_APPEARANCE_SELECTOR" "UNAVAILABLE_ATTRIBUTE"
  ]) @attribute

; Macros

(type_qualifier
  [
    "_Complex"
    "_Nonnull"
    "_Nullable"
    "_Nullable_result"
    "_Null_unspecified"
    "__autoreleasing"
    "__block"
    "__bridge"
    "__bridge_retained"
    "__bridge_transfer"
    "__complex"
    "__kindof"
    "__nonnull"
    "__nullable"
    "__ptrauth_objc_class_ro"
    "__ptrauth_objc_isa_pointer"
    "__ptrauth_objc_super_pointer"
    "__strong"
    "__thread"
    "__unsafe_unretained"
    "__unused"
    "__weak"
  ]) @function.macro.builtin

[ "__real" "__imag" ] @function.macro.builtin

((call_expression function: (identifier) @function.macro)
  (#eq? @function.macro "testassert"))

; Types

(class_declaration (identifier) @type)

(class_interface "@interface" . (identifier) @type superclass: _? @type category: _? @namespace)

(class_implementation "@implementation" . (identifier) @type superclass: _? @type category: _? @namespace)

(protocol_forward_declaration (identifier) @type)

(protocol_reference_list (identifier) @type)

; Ours: a protocol's own name, and a class receiving a message.

(protocol_declaration (identifier) @type)

((message_expression receiver: (identifier) @type)
  (#match? @type "^[A-Z]"))

[
  "BOOL"
  "IMP"
  "SEL"
  "Class"
  "id"
] @type.builtin

; Constants

(property_attribute (identifier) @constant "="?)

[ "__asm" "__asm__" ] @constant.macro

; Properties

(property_implementation "@synthesize" (identifier) @property)

(struct_declaration (struct_declarator (identifier) @property))

(struct_declaration
  (struct_declarator (pointer_declarator declarator: (identifier) @property)))

; Parameters

(method_parameter ":" @method (identifier) @parameter)

(method_parameter declarator: (identifier) @parameter)

(parameter_declaration
  declarator: (function_declarator
                declarator: (parenthesized_declarator
                              (block_pointer_declarator
                                declarator: (identifier) @parameter))))

"..." @parameter.builtin

; Operators

[
  "^"
] @operator

; Literals

(platform) @string.special

(version_number) @number

; Punctuation

"@" @punctuation.special

(protocol_reference_list [ "<" ">" ] @punctuation.bracket)
(parameterized_arguments [ "<" ">" ] @punctuation.bracket)
(generic_specifier [ "<" ">" ] @punctuation.bracket)

; Ours: the `@` of `@"…"` is part of the string.

(string_literal "@" @string)
