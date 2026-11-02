; Written for fastedit: upstream tree-sitter-gitignore ships no highlights
; query. Node names are from its grammar.js.

(comment) @comment

(negation) @operator

[
  (wildcard_char_single)
  (wildcard_chars)
  (wildcard_chars_allow_slash)
] @operator

(directory_separator) @punctuation.delimiter

[
  (directory_separator_escaped)
  (pattern_char_escaped)
  (bracket_char_escaped)
] @escape

(bracket_expr "[" @punctuation.bracket)
(bracket_expr "]" @punctuation.bracket)

(bracket_negation) @operator

(bracket_range "-" @operator)

(bracket_char_class) @constant.builtin
