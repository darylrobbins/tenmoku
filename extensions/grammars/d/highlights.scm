; Two queries inlined in this order, upstream's first:
;
; 1. The grammar's own queries/highlights.scm (MIT, Garrett D'Amore, text in
;    LICENSE) at https://github.com/gdamore/tree-sitter-d commit
;    64f27931b4e6fdd75af1102c79bacbca68a8dacc (v0.9.1), with its own header.
; 2. nvim-treesitter's runtime/queries/d/highlights.scm at commit
;    19bdfc04f40ed2ccb3340591d125f759b0406875 (Apache-2.0, text in
;    LICENSE-APACHE):
;    https://github.com/nvim-treesitter/nvim-treesitter/blob/19bdfc04f40ed2ccb3340591d125f759b0406875/runtime/queries/d/highlights.scm
;
; Changes: this header and the section comments. From upstream's: the
; `(end_file) @text` pattern is dropped (fastedit has no `text` highlight, so
; the text after `__EOF__` stays plain anyway); its interpolation-delimiter
; and identity-expression patterns are moved to the end, after
; nvim-treesitter's, which would otherwise override them as they say they
; mustn't be. From nvim-treesitter's: its `(identifier) @variable`, already
; upstream's, is dropped, since repeated here it would undo upstream's
; identifier patterns before it (template arguments, named arguments, types);
; `@spell` is dropped; `#lua-match?` became `#match?` with the same pattern,
; adding `(?s)` where it has `.` so that it matches a newline as Lua's does
; (multi-line doc comments). Patterns are added at the end, as the comments
; there say. Last pattern wins.

; ---------------------------------------------------------------- upstream

; highlights.scm
;
; Highlighting queries for D code for use by Tree-Sitter.
;
; Copyright 2024 Garrett D'Amore
;
; Distributed under the MIT License.
; (See accompanying file LICENSE.txt or https://opensource.org/licenses/MIT)
; SPDX-License-Identifier: MIT

(string_literal) @string
(int_literal) @number
(float_literal) @number
(char_literal) @number
(identifier) @variable
(at_attribute) @property
(htmlentity) @string.special
(escape_sequence) @string.escape

[
	(lazy)
	(align)
	(extern)
	(static)
	(abstract)
	(final)
	(override)
	(synchronized)
	(auto)
	(scope)
	(gshared)
	(ref)
	(deprecated)
	(nothrow)
	(pure)
	(type_ctor)
] @keyword.storage

(parameter_attribute (return) @keyword.storage)
(parameter_attribute (in) @keyword.storage)
(parameter_attribute (out) @keyword.storage)

(function_declaration (identifier) @function)

(call_expression (identifier) @function)
(call_expression (type (template_instance (identifier) @function)))
(template_arguments (identifier) @variable.parameter)

(named_argument (identifier) @variable.parameter)

[
    (abstract)
    (alias)
    (align)
    (asm)
    (assert)
    (auto)
    (cast)
    (class)
    (const)
    (debug)
    (delegate)
    (delete)
    (deprecated)
    (enum)
    (export)
    (extern)
    (final)
    (function)
    (immutable)
    (import)
    (in)
    (inout)
    (interface)
    (invariant)
    (is)
    (lazy)
    ; "macro" - obsolete
    (mixin)
    (module)
    (new)
    (nothrow)
    (out)
    (override)
    (package)
    (pragma)
    (private)
    (protected)
    (public)
    (pure)
    (ref)
    (scope)
    (shared)
    (static)
    (struct)
    (super)
    (synchronized)
    (template)
    (this)
    (throw)
    (typeid)
    (typeof)
    (union)
    (unittest)
    (version)
    (with)
    (gshared)
    (traits)
    (vector)
    (parameters_)
] @keyword

[
    (break)
    (case)
    (catch)
    (continue)
    (do)
    (default)
    (finally)
    (else)
    (for)
    (foreach)
    (foreach_reverse)
    (goto)
    (if)
    (switch)
    (try)
    (return)
    (while)
] @keyword.control

[
    (not_in)
    (not_is)
    "/="
    "/"
    ".."
    "..."
    "&"
    "&="
    "&&"
    "|"
    "|="
    "||"
    "-"
    "-="
    "--"
    "+"
    "+="
    "++"
    "<"
    "<="
    "<<"
    "<<="
    ">"
    ">="
    ">>="
    ">>>="
    ">>"
    ">>>"
    "!"
    "!="
    "?"
    "$"
    "="
    "=="
    "*"
    "*="
    "%"
    "%="
    "^"
    "^="
    "^^"
    "^^="
    "~"
    "~="
    "@"
    "=>"
] @operator

[
    ";"
    "."
    ":"
    ","
] @punctuation.delimiter

[
    "("
    ")"
    "["
    "]"
    "{"
    "}"
] @punctuation.bracket

[
    (null)
    (true)
    (false)
] @constant.language

(special_keyword) @constant.language

(directive) @keyword.directive
(shebang) @keyword.directive

(comment) @comment

[
    (void)
    (bool)
    (byte)
    (ubyte)
    (char)
    (short)
    (ushort)
    (wchar)
    (dchar)
    (int)
    (uint)
    (long)
    (ulong)
    (real)
    (double)
    (float)
    (size_t)
    (ptrdiff_t)
    (string)
    (cstring)
    (wstring)
    (noreturn)
] @type.builtin

[
    (cent)
    (ucent)
    (ireal)
    (idouble)
    (ifloat)
    (creal)
    (double)
    (cfloat)
] @type.deprecated

(label (identifier) @label)
(goto_statement (goto) @keyword.control (identifier) @label)

; this covers other cases where the identifier can only
; be a type (such as in an is-expression on a constraint)
(type (identifier) @type)

; ---------------------------------------------------------------- nvim-treesitter

; Keywords
[
  (directive)
  (shebang)
] @keyword.directive

[
  (import)
  (module)
] @keyword.import

[
  (alias)
  (asm)
  (class)
  (delegate)
  (delete)
  (enum)
  (interface)
  (invariant)
  (mixin)
  (pragma)
  (struct)
  (template)
  (union)
  (unittest)
  (version)
  (with)
  (traits)
  (vector)
  (parameters_)
  (default)
  (goto)
] @keyword

(function) @keyword.function

(synchronized) @keyword.coroutine

[
  (if)
  (else)
  (switch)
  (case)
  (break)
] @keyword.conditional

[
  (do)
  (for)
  (foreach)
  (foreach_reverse)
  (while)
  (continue)
] @keyword.repeat

(return) @keyword.return

[
  (abstract)
  (deprecated)
  (private)
  (protected)
  (public)
  (package)
  (immutable)
  (final)
  (const)
  (override)
  (static)
] @keyword.modifier

[
  (assert)
  (try)
  (catch)
  (finally)
  (throw)
  (nothrow)
] @keyword.exception

[
  (cast)
  (new)
  (in)
  (is)
  (not_in)
  (not_is)
  (typeid)
  (typeof)
] @keyword.operator

[
  (lazy)
  (align)
  (extern)
  (scope)
  (ref)
  (pure)
  (export)
  (shared)
  (gshared)
  (out)
  (inout)
] @keyword.modifier

(parameter_attribute
  (return) @keyword.modifier)

(parameter_attribute
  (in) @keyword.modifier)

(parameter_attribute
  (out) @keyword.modifier)

(debug) @keyword.debug

; Operators
[
  "/="
  "/"
  ".."
  "&"
  "&="
  "&&"
  "|"
  "|="
  "||"
  "-"
  "-="
  "--"
  "+"
  "+="
  "++"
  "<"
  "<="
  "<<"
  "<<="
  ">"
  ">="
  ">>="
  ">>>="
  ">>"
  ">>>"
  "!"
  "!="
  "$"
  "="
  "=="
  "*"
  "*="
  "%"
  "%="
  "^"
  "^="
  "^^"
  "^^="
  "~"
  "~="
  "@"
] @operator

[
  "exit"
  "success"
  "failure"
  (this)
  (super)
] @variable.builtin

(linkage_attribute
  "("
  _ @variable.builtin
  ")")

; Modules
(module_fqn
  (identifier) @module)

; Attributes
(at_attribute
  (identifier) @attribute)

; Constants
(enum_member
  (identifier) @constant)

(manifest_declarator
  .
  (identifier) @constant)

; Members
(aggregate_body
  (variable_declaration
    (declarator
      (identifier) @variable.member)))

(property_expression
  "."
  (identifier) @variable.member)

(type
  "."
  (identifier) @variable.member)

; Types
(class_declaration
  (class)
  .
  (identifier) @type)

(struct_declaration
  (struct)
  .
  (identifier) @type)

(union_declaration
  (union)
  .
  (identifier) @type)

(enum_declaration
  (enum)
  .
  (identifier) @type)

(alias_declaration
  (alias)
  .
  (identifier) @type)

((identifier) @type
  (#match? @type "^[A-Z].*"))

(type
  .
  (identifier) @type .)

[
  (auto)
  (void)
  (bool)
  (byte)
  (ubyte)
  (char)
  (short)
  (ushort)
  (wchar)
  (dchar)
  (int)
  (uint)
  (long)
  (ulong)
  (real)
  (double)
  (float)
  (cent)
  (ucent)
  (ireal)
  (idouble)
  (ifloat)
  (creal)
  (cfloat)
  (string)
  (dstring)
  (wstring)
] @type.builtin

; Functions
(function_declaration
  (identifier) @function)

(call_expression
  (identifier) @function)

(call_expression
  (type
    (identifier) @function .))

(call_expression
  (property_expression
    (call_expression)
    (identifier) @function .))

; Parameters
(parameter
  (_)
  (identifier) @variable.parameter)

(function_literal
  "("
  (type
    (identifier) @variable.parameter))

; Constructors
(constructor
  (this) @constructor)

(destructor
  (this) @constructor)

(postblit
  .
  (this) @constructor)

; Punctuation
[
  ";"
  "."
  ":"
  ","
  "=>"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

"..." @punctuation.special

; Ternaries
(ternary_expression
  [
    "?"
    ":"
  ] @keyword.conditional.ternary)

; Labels
(label
  (identifier) @label)

(goto_statement
  (identifier) @label)

; Literals
(string_literal) @string

[
  (int_literal)
  (float_literal)
] @number

(char_literal) @character

[
  (true)
  (false)
] @boolean

[
  (null)
  (special_keyword)
] @constant.builtin

; Comments
(comment) @comment

((comment) @comment.documentation
  (#match? @comment.documentation "^///[^/]"))

((comment) @comment.documentation
  (#match? @comment.documentation "^///$"))

((comment) @comment.documentation
  (#match? @comment.documentation "(?s)^/[*][*][^*].*[*]/$"))

((comment) @comment.documentation
  (#match? @comment.documentation "(?s)^/[+][+][^+].*[+]/$"))

; ---------------------------------------------------------------- moved and added

; Moved from upstream's, as the header says.
; these are listed last, because they override keyword queries
(identity_expression (in) @operator)
(identity_expression (is) @operator)

; Added: the code in an interpolated string's `$(...)` isn't string-coloured;
; what has no colour of its own there takes `embedded`.
(interpolation_expression) @embedded

; Moved from upstream's, as the header says.
; The delimiters of an interpolation expression inside an interpolated string.
; Listed after the bracket rule above so the closing ")" is captured here rather
; than as a plain bracket; this keeps the opening "$(" and closing ")" the same
; color instead of the opener inheriting the surrounding string color.
(interpolation_expression
    "$(" @punctuation.special
    ")" @punctuation.special)

; Added: the grammar parses a call's dotted callee as a type, so upstream's
; `(type (identifier) @type)` colours `store` in `store.put(item)` as a type.
; The callee's first part takes `variable`, as it does in nvim-treesitter's
; query, unless it's capitalised (`Exception.init()`), when it stays `type`.
(call_expression
  (type
    .
    (identifier) @variable
    .
    (identifier))
  (#not-match? @variable "^[A-Z]"))
