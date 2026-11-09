; From tree-sitter-grammars/tree-sitter-glsl queries/highlights.scm at
; 24a6c8ef698e4480fecf8340d771fbcb5de8fbb4, licence MIT (see LICENSE). Upstream's
; generate-highlight.sh builds that file from tree-sitter-c's queries/highlights.scm
; (MIT, same licence and holder) and nvim-treesitter's queries/glsl/highlights.scm
; (Apache-2.0, see LICENSE-APACHE), both taken at their master of the day: no
; revision of either is recorded upstream. The first part is the C patterns, the
; GLSL qualifier and `gl_` builtin patterns are nvim-treesitter's. Its
; `; inherits: c` line is dropped. Changes: `#lua-match?` is `#match?`, captures
; are renamed to fe-syntax's vocabulary, `true`/`false` and an escape pattern are
; added, and the GLSL patterns come before `(identifier)`, since earlier patterns win.
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

[
  "in"
  "out"
  "inout"
  "uniform"
  "shared"
  "layout"
  "attribute"
  "varying"
  "buffer"
  "coherent"
  "readonly"
  "writeonly"
  "precision"
  "highp"
  "mediump"
  "lowp"
  "centroid"
  "sample"
  "patch"
  "smooth"
  "flat"
  "noperspective"
  "invariant"
  "precise"
] @keyword

"subroutine" @keyword
(extension_storage_class) @keyword

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

"." @punctuation.delimiter
";" @punctuation.delimiter

(string_literal) @string
(escape_sequence) @escape
(system_lib_string) @string

(null) @constant
(true) @constant.builtin
(false) @constant.builtin
(number_literal) @number
(char_literal) @number

(call_expression
  function: (identifier) @function)
(call_expression
  function: (field_expression
    field: (field_identifier) @function))
(function_declarator
  declarator: (identifier) @function)
(preproc_function_def
  name: (identifier) @function.macro)

(field_identifier) @property
(statement_identifier) @label
(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin

((identifier) @variable.builtin
  (#match? @variable.builtin "^gl_"))

((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\\d_]*$"))

(identifier) @variable

(comment) @comment
