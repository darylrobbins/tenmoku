; Adapted from uben0/tree-sitter-typst's queries/typst/highlights.scm at commit
; 46cf4ded12ee974a70bf8457263b67ad7ee0379d (MIT, see LICENSE). Captures renamed
; to fe-syntax's vocabulary.

(call
  item: (ident) @function)
(call
  item: (field field: (ident) @function.method))
(tagged field: (ident) @tag)
(field field: (ident) @property)
(comment) @comment

; CONTROL
(let "let" @keyword)
(branch ["if" "else"] @keyword)
(while "while" @keyword)
(for ["for" "in"] @keyword)
(import "import" @keyword)
(as "as" @keyword)
(include "include" @keyword)
(show "show" @keyword)
(set "set" @keyword)
(return "return" @keyword)
(flow ["break" "continue"] @keyword)

; OPERATOR
(in ["in" "not"] @keyword)
(context "context" @keyword)
(and "and" @keyword)
(or "or" @keyword)
(not "not" @keyword)
(sign ["+" "-"] @operator)
(add "+" @operator)
(sub "-" @operator)
(mul "*" @operator)
(div "/" @operator)
(cmp ["==" "<=" ">=" "!=" "<" ">"] @operator)
(fraction "/" @operator)
(fac "!" @operator)
(attach ["^" "_"] @operator)
(wildcard) @operator

; VALUE
(raw_blck "```" @operator) @markup.raw
(raw_span "`" @operator) @markup.raw
(raw_blck lang: (ident) @tag)
(label) @tag
(ref) @tag
(number) @number
(string) @string
(content ["[" "]"] @operator)
(bool) @constant.builtin
(none) @constant.builtin
(auto) @constant.builtin
; The built-in types, as in `type(x) == int`.
((ident) @type.builtin
  (#any-of? @type.builtin
    "int" "float" "str" "bool" "array" "dictionary" "content" "length" "angle"
    "ratio" "relative" "fraction" "color" "gradient" "regex" "datetime"
    "duration" "bytes" "version" "function" "type" "label" "decimal" "symbol"
    "selector" "location" "stroke" "alignment" "direction"))
(ident) @variable

; MARKUP
(item "-" @punctuation.special)
(term ["/" ":"] @punctuation.special)
(heading) @markup.heading
(url) @markup.link.url
(symbol) @constant
(shorthand) @constant.builtin
(quote) @string
(align) @operator
(letter) @constant
(linebreak) @constant.builtin

(math "$" @operator)
"#" @operator
"end" @operator

(escape) @escape
["(" ")" "{" "}"] @punctuation.bracket
["," ";" ".." ":" "sep"] @punctuation.delimiter
"assign" @punctuation
(field "." @punctuation)
