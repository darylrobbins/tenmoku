;; Adapted from tree-sitter-erlang's queries/highlights.scm (Apache-2.0, same
;; licence as the grammar) at commit 62c82e67b73e13f3ef06e4c8c07ff035a8235d30:
;; https://github.com/WhatsApp/tree-sitter-erlang/blob/62c82e67b73e13f3ef06e4c8c07ff035a8235d30/queries/highlights.scm
;; Changes: this header; captures renamed to fe-syntax's vocabulary. Last
;; pattern wins, as upstream; type names are @type, not @function, and floats are @number.
;;
;; Copyright (c) Facebook, Inc. and its affiliates.
;;
;; Licensed under the Apache License, Version 2.0 (the "License");
;; you may not use this file except in compliance with the License.
;; You may obtain a copy of the License at
;;
;;     http://www.apache.org/licenses/LICENSE-2.0
;;
;; Unless required by applicable law or agreed to in writing, software
;; distributed under the License is distributed on an "AS IS" BASIS,
;; WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
;; See the License for the specific language governing permissions and
;; limitations under the License.
;; ---------------------------------------------------------------------

;; Based initially on the contents of https://github.com/WhatsApp/tree-sitter-erlang/issues/2 by @Wilfred
;; and https://github.com/the-mikedavis/tree-sitter-erlang/blob/main/queries/highlights.scm
;;
;; The tests are also based on those in
;; https://github.com/the-mikedavis/tree-sitter-erlang/tree/main/test/highlight
;;

;; Last match wins in this file.
;; As of https://github.com/tree-sitter/tree-sitter/blob/master/CHANGELOG.md#breaking-1

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Primitive types
(string) @string
(char) @constant
(integer) @number
(float) @number
(var) @variable
(atom) @string.special

;;; Comments
((var) @comment
 (#match? @comment "^_"))

(dotdotdot) @comment
(comment) @comment

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Functions
(fa fun: (atom) @function)
(type_name name: (atom) @type)
(call expr: (atom) @function)
(function_clause name: (atom) @function)
(internal_fun fun: (atom) @function)

;; This is a fudge, we should check that the operator is '/'
;; But our grammar does not (currently) provide it
(binary_op_expr lhs: (atom) @function rhs: (integer))

;; Others
(remote_module module: (atom) @module)
(remote fun: (atom) @function)
(macro_call_expr name: (var) @constant)
(macro_call_expr name: (var) @keyword args: (_) )
(macro_call_expr name: (atom) @keyword)
(record_field_name name: (atom) @property)
(record_name name: (atom) @type)

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Attributes

;; module attribute
(module_attribute
  name: (atom) @module)

;; behaviour
(behaviour_attribute name: (atom) @module)

;; export

;; Import attribute
(import_attribute
    module: (atom) @module)

;; export_type

;; optional_callbacks

;; compile
(compile_options_attribute
    options: (tuple
      expr: (atom)
      expr: (list
        exprs: (binary_op_expr
          lhs: (atom)
          rhs: (integer)))))

;; file attribute

;; record
(record_decl name: (atom) @type)
(record_decl name: (macro_call_expr name: (var) @constant))
(record_field name: (atom) @property)

;; type alias

;; opaque

;; Spec attribute
(spec fun: (atom) @function)
(spec
  module: (module name: (atom) @module)
  fun: (atom) @function)

;; callback
(callback fun: (atom) @function)

;; wild attribute
(wild_attribute name: (attr_name name: (atom) @keyword))

;; fun decl

;; include/include_lib

;; ifdef/ifndef
(pp_ifdef name: (_) @keyword)
(pp_ifndef name: (_) @keyword)

;; define
(pp_define
    lhs: (macro_lhs
      name: (var) @constant))
(pp_define
    lhs: (macro_lhs
      name: (_) @keyword
      args: (var_args args: (var))))


;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Reserved words
[ "after"
  "and"
  "band"
  "begin"
  "behavior"
  "behaviour"
  "bnot"
  "bor"
  "bsl"
  "bsr"
  "bxor"
  "callback"
  "case"
  "catch"
  "compile"
  "define"
  "div"
  "elif"
  "else"
  "end"
  "endif"
  "export"
  "export_type"
  "file"
  "fun"
  "if"
  "ifdef"
  "ifndef"
  "import"
  "include"
  "include_lib"
  "module"
  "of"
  "opaque"
  "optional_callbacks"
  "or"
  "receive"
  "record"
  "spec"
  "try"
  "type"
  "undef"
  "unit"
  "when"
  "xor"] @keyword

["andalso" "orelse"] @keyword.operator

;; Punctuation
["," "." ";"] @punctuation.delimiter
["(" ")" "{" "}" "[" "]" "<<" ">>"] @punctuation.bracket

;; Operators
["!"
 "->"
 "<-"
 "#"
 "::"
 ":>"
 "|"
 ":"
 "="
 "||"

 "+"
 "-"
 "bnot"
 "not"

 "/"
 "*"
 "div"
 "rem"
 "band"
 "and"

 "+"
 "-"
 "bor"
 "bxor"
 "bsl"
 "bsr"
 "or"
 "xor"

 "++"
 "--"

 "=="
 "/="
 "=<"
 "<"
 ">="
 ">"
 "=:="
 "=/="
 ] @operator
