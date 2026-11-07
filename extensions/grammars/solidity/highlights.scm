; From tree-sitter-solidity's queries/highlights-nvim.scm at commit
; 048fe686cb1fde267243739b8bdbec8fc3a55272 (https://github.com/JoranHonig/tree-sitter-solidity), MIT;
; its licence is in LICENSE. Adapted: dropped the #offset! string-path pattern,
; Neovim's @spell, and @character.special (now @operator); #lua-match? is #match?.
; Later patterns win (earlier_patterns_win = false).

; Pragma
[
  "pragma"
  "solidity"
] @keyword.directive

(solidity_pragma_token
  "||" @string.special.symbol)

(solidity_pragma_token
  "-" @string.special.symbol)

(solidity_version_comparison_operator) @operator

(solidity_version) @string.special

; Literals
[
  (string)
  (hex_string_literal)
  (unicode_string_literal)
  (yul_string_literal)
] @string

[
  (number_literal)
  (yul_decimal_number)
  (yul_hex_number)
] @number

[
  (boolean_literal)
  (yul_boolean)
] @boolean

; Variables
[
  (identifier)
  (yul_identifier)
] @variable

; Types
(type_name
  (identifier) @type)

(type_name
  (user_defined_type
    (identifier) @type))

(type_name
  "mapping" @function.builtin)

[
  (primitive_type)
  (number_unit)
] @type.builtin

(user_defined_type
  (identifier) @type)

(contract_declaration
  name: (identifier) @type)

(interface_declaration
  name: (identifier) @type)

(library_declaration
  name: (identifier) @type)

(event_definition
  name: (identifier) @type)

(struct_expression
  type: (expression (identifier) @type))

; using Math for uint256;
(type_alias
  (identifier) @type)

(struct_declaration
  name: (identifier) @type)

(struct_member
  name: (identifier) @variable.member)

(enum_declaration
  name: (identifier) @type)

(emit_statement
  .
  (expression (identifier)) @type)

; Handles ContractA, ContractB in function foo() override(ContractA, contractB) {}
(override_specifier
  (user_defined_type) @type)

; Functions and parameters
(function_definition
  name: (identifier) @function)

(modifier_definition
  name: (identifier) @function)

(yul_evm_builtin) @function.builtin

; Use constructor coloring for special functions
(constructor_definition
  "constructor" @constructor)

(modifier_invocation
  (identifier) @function)

; Handles expressions like g();
(call_expression
  .
  (expression (identifier) @function.call))

; Function parameters
(event_parameter
  name: (_) @variable.parameter)

(parameter
  name: (_) @variable.parameter)

; Yul functions
(yul_function_call
  function: (yul_identifier) @function.call)

; Yul function parameters
(yul_function_definition
  .
  (yul_identifier) @function
  (yul_identifier) @variable.parameter)

(meta_type_expression
  "type" @keyword)

(member_expression
  property: (_) @variable.member)

(call_struct_argument
  name: (_) @variable.member)

; Handles expressions like structVariable.g(); after the member_expression
; pattern above, which it must override. Upstream captured the first identifier
; (the object, not the method); this takes the property.
(call_expression
  .
  (expression(member_expression
    property: (_) @function.method.call)))

(struct_field_assignment
  name: (identifier) @variable.member)

(enum_value) @constant

; Account({balance: 0}): a struct constructor call, after the call patterns.
(call_expression
  function: (expression (identifier) @type)
  (call_argument (call_struct_argument)))

; Keywords
[
  "contract"
  "abstract"
  "interface"
  "library"
  "is"
  "struct"
  "enum"
  "event"
  "assembly"
  "emit"
  "override"
  "modifier"
  "var"
  "let"
  "emit"
  "error"
  "fallback"
  "receive"
  (virtual)
] @keyword

; FIXME: update grammar
; (block_statement "unchecked" @keyword)
(event_parameter
  "indexed" @keyword)

[
  "public"
  "internal"
  "private"
  "external"
  "pure"
  "view"
  "payable"
  (immutable)
] @keyword.modifier

[
  "memory"
  "storage"
  "calldata"
  "constant"
] @keyword.modifier

[
  "for"
  "while"
  "do"
  "break"
  "continue"
] @keyword.repeat

[
  "if"
  "else"
  "switch"
  "case"
  "default"
] @keyword.conditional

(ternary_expression
  "?" @keyword.conditional.ternary
  ":" @keyword.conditional.ternary)

[
  "try"
  "catch"
  "revert"
] @keyword.exception

[
  "return"
  "returns"
  (yul_leave)
] @keyword.return

"function" @keyword.function

[
  "import"
  "using"
] @keyword.import

(import_directive
  "as" @keyword.import)

(import_directive
  "from" @keyword.import)

; Punctuation
[
  "{"
  "}"
] @punctuation.bracket

[
  "["
  "]"
] @punctuation.bracket

[
  "("
  ")"
] @punctuation.bracket

[
  "."
  ","
  ":"
  ; FIXME: update grammar
  ; (semicolon)
  "->"
  "=>"
] @punctuation.delimiter

; Operators
[
  "&&"
  "||"
  ">>"
  "<<"
  "&"
  "^"
  "|"
  "+"
  "-"
  "*"
  "/"
  "%"
  "**"
  "="
  "<"
  "<="
  "=="
  "!="
  ">="
  ">"
  "!"
  "~"
  "-"
  "+"
  "++"
  "--"
  ":="
] @operator

[
  "delete"
  "new"
] @keyword.operator

(import_directive
  "*" @operator)

; Comments
(comment) @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^///[^/]"))

((comment) @comment.documentation
  (#match? @comment.documentation "^///$"))

((comment) @comment.documentation
  (#match? @comment.documentation "^/[*][*][^*].*[*]/$"))
