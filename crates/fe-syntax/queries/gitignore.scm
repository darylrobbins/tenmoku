; Ours, replacing the vendored query (upstream ships none; node names are from
; vendor/tree-sitter-gitignore/grammar.js). Later patterns win here, so the
; negated-line rules at the end override the general ones.
;
; One query serves .gitignore, .dockerignore, .npmignore and the rest.
;
;   @comment               comment lines
;   @string                name characters on an ordinary (ignoring) line
;   @variable              the "!" and name characters on a negated line, so a
;                          re-include reads differently from an ignore
;   @punctuation.special   a leading "/" (anchor) and a trailing "/" (directory
;                          only): the slashes that change what a pattern means
;   @punctuation.delimiter an inner "/"
;   @operator              "*", "?", and the "!" of a bracket class
;   @keyword               "**", which crosses directories
;   @escape                backslash escapes
;   @punctuation.bracket   "[" and "]"; @constant.builtin for [:alpha:] classes

(comment) @comment

(pattern_char) @string

(negation) @operator

(wildcard_char_single) @operator
(wildcard_chars) @operator
(wildcard_chars_allow_slash) @keyword

(directory_separator) @punctuation.delimiter

; The grammar fields every separator between names relative_flag, so anchor to
; the first child (or the one right after "!") to find the leading one.
(pattern
  .
  [
    (directory_separator)
    (directory_separator_escaped)
  ] @punctuation.special)

(pattern
  (negation)
  .
  [
    (directory_separator)
    (directory_separator_escaped)
  ] @punctuation.special)

(pattern
  directory_flag: [
    (directory_separator)
    (directory_separator_escaped)
  ] @punctuation.special)

[
  (directory_separator_escaped)
  (pattern_char_escaped)
  (bracket_char_escaped)
] @escape

(bracket_expr "[" @punctuation.bracket)
(bracket_expr "]" @punctuation.bracket)
(bracket_char) @string
(bracket_negation) @operator
(bracket_range "-" @operator)
(bracket_char_class) @constant.builtin

; A negated line is styled as a unit.
(pattern
  (negation) @variable
  (pattern_char) @variable)
