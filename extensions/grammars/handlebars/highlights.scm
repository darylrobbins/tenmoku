; From tree-sitter-handlebars, https://github.com/bennypowers/tree-sitter-handlebars,
; commit 6cf6821144efca0ba9327497f0523f5ebc04abef, path queries/highlights.scm,
; license MIT. Change: adds the helper name of a call
; (an expression or subexpression with arguments) as @function.call at the
; end; the block open/close keyword is anchored to the callee path (not
; its arguments); adds `#*inline` and the text inside `[...]` segments.
; Later patterns win.
;
; Known limits of tree-sitter-handlebars at that commit, which this
; extension inherits (a grammar swap should re-check each). Found by adding
; each form to test/sample.hbs: the host tree then holds an ERROR or MISSING
; node, which the parity test rejects, so the forms are kept out of the sample.
; The upstream repository has no issue open for any of them (checked 2026-11-09).
; - Mustache inverse sections `{{^x}}...{{/x}}`: the grammar has no inverse
;   section rule and no `^` token (grammar.json), and the sample then fails to
;   parse cleanly, so the section is not coloured as a block.
; - Bare literals as a whole mustache, `{{true}}` and `{{42}}`: the sample
;   then fails to parse cleanly; as arguments, `{{f true 42}}`, they parse and
;   are coloured.
; - Segment literals with spaces, `{{a.[weird key]}}`: the sample then
;   fails to parse cleanly; `{{a.[property-name]}}` parses and is coloured.
; `.mustache` files stay mapped to this grammar.

(comment) @comment

; `#*inline` is an unnamed regex token with no node name to match, so take
; the whole node and let the later {{ }} ~ and string patterns recolour theirs.
(inline_partial_open) @keyword

(string_literal) @string
(number_literal) @number
(boolean_literal) @boolean
(null_literal) @constant.builtin

(identifier) @variable

(hash_pair
  (identifier) @property)

(block_open
  "#" @keyword
  . (path_expression (identifier) @keyword))

(block_close
  "/" @keyword
  . (path_expression (identifier) @keyword))

(else_block
  "else" @keyword)

(partial
  ">" @keyword)

(partial_block_open
  "#>" @keyword)

(decorator
  "*" @keyword)

(block_decorator_open
  "#*" @keyword)

(expression
  "&" @operator)

(subexpression
  "(" @punctuation.bracket
  ")" @punctuation.bracket)

(block_params
  "as" @keyword
  "|" @punctuation.delimiter)

(path_expression
  "this" @variable.builtin)

(path_expression
  "." @variable.builtin)

(segment_literal) @string.special

(segment_literal
  "[" @punctuation.bracket
  "]" @punctuation.bracket)

"{{" @punctuation.special
"}}" @punctuation.special
"{{{" @punctuation.special
"}}}" @punctuation.special
"{{{{" @punctuation.special
"}}}}" @punctuation.special
"{{{{/" @punctuation.special
"~" @operator
"=" @operator
"." @punctuation.delimiter

; A helper call: the name before its arguments (`{{fmt price}}`), not a lone
; lookup (`{{title}}`).
(expression
  . (path_expression . (identifier) @function.call .)
  . [(path_expression) (string_literal) (number_literal) (boolean_literal)
     (null_literal) (subexpression) (hash_pair)])
(subexpression
  . (path_expression . (identifier) @function.call .)
  . [(path_expression) (string_literal) (number_literal) (boolean_literal)
     (null_literal) (subexpression) (hash_pair)])
