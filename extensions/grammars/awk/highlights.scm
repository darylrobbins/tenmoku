; From Beaglefoot/tree-sitter-awk queries/highlights.scm (v0.7.2) at
; 34bbdc7cce8e803096f47b625979e34c1be38127, licence MIT (see LICENSE). Adapted:
; captures renamed to fe-syntax's vocabulary, BEGIN/END and the built-in
; functions and escapes given their own captures, and `(pattern)` (any
; condition, not just BEGIN/END) no longer coloured as a namespace.

(ns_qualified_name (namespace) @module)
(ns_qualified_name "::" @operator)

(func_def name: (_ (identifier) @function) @function)
(func_call name: (_ (identifier) @function) @function)

[
  "and" "asort" "asorti" "bindtextdomain" "compl" "cos" "dcgettext"
  "dcngettext" "exp" "gensub" "gsub" "index" "int" "isarray" "length" "log"
  "lshift" "match" "mktime" "or" "patsplit" "rand" "rshift" "sin" "split"
  "sprintf" "sqrt" "srand" "strftime" "strtonum" "sub" "substr" "systime"
  "tolower" "toupper" "typeof" "xor"
] @function.builtin

[
  (identifier)
  (field_ref)
] @variable
(field_ref (_) @variable)

(string) @string
(escape_sequence) @escape
(number) @number
(regex) @string.special
(regex_pattern) @string.special
(comment) @comment

[
  "BEGIN"
  "END"
  "BEGINFILE"
  "ENDFILE"
  "function"
  "func"
  "print"
  "printf"
  "if"
  "else"
  "do"
  "while"
  "for"
  "in"
  "delete"
  "return"
  "exit"
  "switch"
  "case"
  "default"
  (break_statement)
  (continue_statement)
  (next_statement)
  (nextfile_statement)
  (getline_input)
  (getline_file)
] @keyword

[
  "@include"
  "@load"
  "@namespace"
] @keyword

(binary_exp [
  "^"
  "**"
  "*"
  "/"
  "%"
  "+"
  "-"
  "<"
  ">"
  "<="
  ">="
  "=="
  "!="
  "~"
  "!~"
  "in"
  "&&"
  "||"
] @operator)

(unary_exp [
  "!"
  "+"
  "-"
] @operator)

(assignment_exp [
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "^="
] @operator)

(ternary_exp [
  "?"
  ":"
] @operator)

(update_exp [
  "++"
  "--"
] @operator)

(redirected_io_statement [
  ">"
  ">>"
] @operator)

(piped_io_statement [
  "|"
  "|&"
] @operator)

[
  ";"
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
