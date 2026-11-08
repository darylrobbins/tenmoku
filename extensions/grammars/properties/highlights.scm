; Upstream: tree-sitter-grammars/tree-sitter-properties queries/highlights.scm at
; 6310671b24d4e04b803577b1c675d765cbd5773b, under the grammar's MIT licence (LICENSE).
; Changed: the secret provider key (`${vault::path}`) is @string.special, not
; upstream's @function: Java properties has no functions, so none are invented.

(comment) @comment

(key) @property

(value) @string

(value (escape) @string.escape)

((index) @number
  (#match? @number "^[0-9]+$"))

((substitution (key) @constant)
  (#match? @constant "^[A-Z0-9_]+"))

(substitution
  (key) @string.special
  "::" @punctuation.special
  (secret) @embedded)

(property [ "=" ":" ] @operator)

[ "${" "}" ] @punctuation.special

(substitution ":" @punctuation.special)

[ "[" "]" ] @punctuation.bracket

[ "." "\\" ] @punctuation.delimiter
