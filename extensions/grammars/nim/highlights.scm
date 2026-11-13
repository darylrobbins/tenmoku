; Adapted from metacraft-labs/tree-sitter-nim's queries/highlights.scm at
; commit 67cd55401207fa04c0a108fe8535dcddb297215e (MIT, the grammar's own
; licence and holder, text in LICENSE):
; https://github.com/metacraft-labs/tree-sitter-nim/blob/67cd55401207fa04c0a108fe8535dcddb297215e/queries/highlights.scm
;
; The grammar is tenmoku-editor/tree-sitter-nim at
; d7408ccad04d1ae23d387ba6d1413391d76e95ab, a fork of metacraft-labs' at that
; commit with Tenmoku's case/of patch: `of`/`elif`/`else` branches may sit at
; `case`'s column (NEP-1) and `elif` gets an elif_branch node. The query is
; unaffected: those keywords are matched as tokens wherever they appear.
;
; Changes: this header; the lambda pattern that captured a whole parameter
; list as @punctuation.bracket is dropped; "except" is only an exception
; keyword (it was also an import one, which won); "nil" leaves the keyword
; list, so nil_literal stays @constant.builtin; "addr" joins the keywords and
; "pointer" the built-in types; a doc comment (`##`, `##[ ]##`) is
; @comment.documentation; the call patterns are rewritten against the
; grammar's tree (upstream's captured receivers and fields as calls, and its
; command and block-call patterns never matched): they now anchor on the
; callee, and a dotted name that isn't called is @property. Last pattern wins.

; Nim Tree-sitter Syntax Highlighting Queries
; ===========================================

; Comments
; --------
(comment) @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^##"))

; Literals
; --------
(integer_literal) @number
(float_literal) @number.float

(string_literal) @string
(raw_string_literal) @string
(triple_string_literal) @string
(char_literal) @character

(boolean_literal) @constant.builtin
(nil_literal) @constant.builtin

; Generalized string literals (e.g., re"pattern", fmt"hello")
(generalized_string) @string.special

; Identifiers
; -----------
(identifier) @variable

; Quoted identifiers (e.g., `+`, `my-var`)
(quoted_identifier) @variable

; Qualified identifiers (e.g., system.echo)
(qualified_identifier
  (symbol) @module
  .
  (symbol) @variable)

; Constants - UPPER_CASE identifiers
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

; Type names - PascalCase identifiers (in type contexts)
(type_expr
  (qualified_identifier
    (symbol
      (identifier) @type)))

(generic_type
  name: (qualified_identifier
    (symbol
      (identifier) @type)))

; Function Definitions
; --------------------
(routine_definition
  kind: (routine_kind) @keyword.function
  name: (exported_symbol
    (symbol
      (identifier) @function.definition)))

(routine_definition
  kind: (routine_kind) @keyword.function
  name: (exported_symbol
    (symbol
      (quoted_identifier) @function.definition)))

; Routine headers (forward declarations)
(routine_header
  kind: (routine_kind) @keyword.function
  name: (exported_symbol
    (symbol
      (identifier) @function.definition)))

; Function Calls
; --------------
; postfix_expr is flat: a primary, then its suffixes (call, dot, index, ...),
; so the anchors below pick the callee: the part right before the call
; suffix, or the last part of a command's or a block call's callee, either
; one past explicit generic arguments (initTable[string, int](), an
; index_suffix; syntactically the same as calling an element, so xs in
; xs[0](1) is a callee too). A receiver or a field further left stays a
; variable or a property.
(call_suffix
  "(" @punctuation.bracket
  ")" @punctuation.bracket)

; Field access: x.name (a call shape below makes it a method)
(dot_suffix
  (symbol_or_keyword
    (symbol
      (identifier) @property)))

; Direct call: foo(x), initTable[string, int](), foo do (y): ...,
; let y = foo: ...
(postfix_expr
  .
  (postfixable_primary
    (symbol
      [(identifier) (quoted_identifier)] @function.call))
  .
  [(call_suffix) (do_block) (colon_block_suffix)])

(postfix_expr
  .
  (postfixable_primary
    (symbol
      [(identifier) (quoted_identifier)] @function.call))
  .
  (index_suffix)
  .
  [(call_suffix) (do_block) (colon_block_suffix)])

; Method call: x.foo(), x.to[int](), x.foo do (y): ...
(postfix_expr
  (dot_suffix
    (symbol_or_keyword
      (symbol
        (identifier) @function.method)))
  .
  [(call_suffix) (do_block) (colon_block_suffix)])

(postfix_expr
  (dot_suffix
    (symbol_or_keyword
      (symbol
        (identifier) @function.method)))
  .
  (index_suffix)
  .
  [(call_suffix) (do_block) (colon_block_suffix)])

; Command call: echo "hello", inv.add item, newSeq[int] 3,
; withLog "x": body. The callee ends the function: postfix_expr. (Not an
; optional (index_suffix)? before the closing anchor: tree-sitter then drops
; that anchor and takes the receiver of x.foo[T] y.)
(command_expr
  function: (postfix_expr
    .
    (postfixable_primary
      (symbol
        [(identifier) (quoted_identifier)] @function.call))
    .))

(command_expr
  function: (postfix_expr
    .
    (postfixable_primary
      (symbol
        [(identifier) (quoted_identifier)] @function.call))
    .
    (index_suffix)
    .))

(command_expr
  function: (postfix_expr
    (dot_suffix
      (symbol_or_keyword
        (symbol
          (identifier) @function.method)))
    .))

(command_expr
  function: (postfix_expr
    (dot_suffix
      (symbol_or_keyword
        (symbol
          (identifier) @function.method)))
    .
    (index_suffix)
    .))

; Call with blocks and no arguments: foo: body, foo[T]: body,
; x.map do (y): y + 1. Its call: is an expression wrapping the postfix_expr
; one node per precedence level.
(call_with_blocks
  call: (expression . (infix_expression . (assign_expr . (op2_expr .
    (or_expr . (and_expr . (cmp_expr . (range_expr . (amp_expr .
    (add_expr . (mul_expr . (pow_expr . (prefix_expr .
      (postfix_expr
        .
        (postfixable_primary
          (symbol
            [(identifier) (quoted_identifier)] @function.call))
        .)
    .) .) .) .) .) .) .) .) .) .) .) .) .))

(call_with_blocks
  call: (expression . (infix_expression . (assign_expr . (op2_expr .
    (or_expr . (and_expr . (cmp_expr . (range_expr . (amp_expr .
    (add_expr . (mul_expr . (pow_expr . (prefix_expr .
      (postfix_expr
        .
        (postfixable_primary
          (symbol
            [(identifier) (quoted_identifier)] @function.call))
        .
        (index_suffix)
        .)
    .) .) .) .) .) .) .) .) .) .) .) .) .))

(call_with_blocks
  call: (expression . (infix_expression . (assign_expr . (op2_expr .
    (or_expr . (and_expr . (cmp_expr . (range_expr . (amp_expr .
    (add_expr . (mul_expr . (pow_expr . (prefix_expr .
      (postfix_expr
        (dot_suffix
          (symbol_or_keyword
            (symbol
              (identifier) @function.method)))
        .)
    .) .) .) .) .) .) .) .) .) .) .) .) .))

(call_with_blocks
  call: (expression . (infix_expression . (assign_expr . (op2_expr .
    (or_expr . (and_expr . (cmp_expr . (range_expr . (amp_expr .
    (add_expr . (mul_expr . (pow_expr . (prefix_expr .
      (postfix_expr
        (dot_suffix
          (symbol_or_keyword
            (symbol
              (identifier) @function.method)))
        .
        (index_suffix)
        .)
    .) .) .) .) .) .) .) .) .) .) .) .) .))

; Parameters
; ----------
(param_decl
  (symbol
    (identifier) @variable.parameter))

(param_decl
  modifier: (param_modifier) @keyword.modifier)

; Generic parameters
(generic_param
  (symbol
    (identifier) @type.parameter))

; For loop variables
(for_pattern
  (symbol
    (identifier) @variable))

; Type Definitions
; ----------------
(type_def
  name: (exported_symbol
    (symbol
      (identifier) @type.definition)))

; Enum fields
(enum_field_def
  name: (exported_symbol
    (symbol
      (identifier) @constant)))

; Object fields
(object_field
  name: (exported_symbol
    (symbol
      (identifier) @property)))

; Tuple fields
(tuple_field
  (symbol
    (identifier) @property))

; Variable Definitions
; --------------------
(decl_def
  (exported_symbol
    (symbol
      (identifier) @variable.definition)))

; Tuple unpacking
(tuple_decl
  (exported_symbol
    (symbol
      (identifier) @variable.definition)))

; Operators
; ---------
(add_operator) @operator
(mul_operator) @operator
(pow_operator) @operator
(cmp_operator) @operator
(amp_operator) @operator
(range_operator) @operator
(arrow_operator) @operator
(assign_operator) @operator
(or_operator) @operator
(and_operator) @operator
(op2_operator) @operator
(type_cmp_operator) @operator
(unary_operator) @operator
(custom_prefix_operator) @operator

; Assignment
(assignment_stmt
  "=" @operator)

; Punctuation
; -----------
"(" @punctuation.bracket
")" @punctuation.bracket
"[" @punctuation.bracket
"]" @punctuation.bracket
"{" @punctuation.bracket
"}" @punctuation.bracket

"," @punctuation.delimiter
";" @punctuation.delimiter
":" @punctuation.delimiter
"." @punctuation.delimiter

; Pragmas
; -------
(pragma
  "{." @punctuation.special
  ".}" @punctuation.special)

(pragma_item
  (symbol
    (identifier) @attribute))

; Export marker
(exported_symbol
  "*" @punctuation.special)

; Keywords
; --------
; Control flow
[
  "if"
  "elif"
  "else"
  "when"
  "case"
  "of"
] @keyword.conditional

[
  "while"
  "for"
  "in"
] @keyword.repeat

[
  "try"
  "except"
  "finally"
  "raise"
] @keyword.exception

[
  "break"
  "continue"
  "return"
  "yield"
] @keyword.return

; Definitions
[
  "proc"
  "func"
  "method"
  "iterator"
  "macro"
  "template"
  "converter"
] @keyword.function

[
  "type"
  "object"
  "enum"
  "concept"
  "tuple"
  "distinct"
] @keyword.type

[
  "var"
  "let"
  "const"
  "using"
] @keyword.storage

; Modifiers
[
  "ref"
  "ptr"
  "static"
  "lent"
  "sink"
  "out"
] @keyword.modifier

; Imports/exports
[
  "import"
  "export"
  "include"
  "from"
  "as"
] @keyword.import

; Other keywords
[
  "block"
  "defer"
  "do"
  "discard"
  "asm"
  "bind"
  "mixin"
  "cast"
  "addr"
] @keyword

; Operators as keywords
[
  "and"
  "or"
  "xor"
  "not"
  "div"
  "mod"
  "shl"
  "shr"
  "is"
  "isnot"
  "notin"
] @keyword.operator

; Built-in types (common ones)
((identifier) @type.builtin
  (#any-of? @type.builtin
    "int" "int8" "int16" "int32" "int64"
    "uint" "uint8" "uint16" "uint32" "uint64"
    "float" "float32" "float64"
    "bool" "char" "string" "cstring"
    "void" "auto" "any" "pointer"
    "seq" "array" "set" "Table" "HashSet"
    "openArray" "varargs" "typedesc" "range"
    "Natural" "Positive" "BiggestInt" "BiggestFloat"
    "SomeInteger" "SomeFloat" "SomeNumber" "SomeOrdinal"))

; Built-in functions (common ones)
((identifier) @function.builtin
  (#any-of? @function.builtin
    "echo" "debugEcho" "quit" "assert" "doAssert"
    "len" "high" "low" "sizeof" "typeof" "repr"
    "add" "del" "delete" "insert" "pop" "contains"
    "newSeq" "newString" "new" "alloc" "dealloc" "realloc"
    "inc" "dec" "succ" "pred" "abs" "min" "max" "clamp"
    "ord" "chr" "toInt" "toFloat" "toBool"
    "defined" "declared" "compiles" "compileTime"
    "likely" "unlikely"
    "move" "sink" "lent"
    "staticRead" "staticExec" "gorge" "slurp"))

; Special identifiers
((identifier) @variable.builtin
  (#any-of? @variable.builtin "result" "self" "this"))
