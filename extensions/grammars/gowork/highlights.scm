; From tree-sitter-go-work at 949a8a470559543857a62102c84700d291fc984c,
; https://github.com/omertuc/tree-sitter-go-work, path queries/highlights.scm,
; license MIT. Changes, matching gomod's: versions are @number rather than
; @string; adds module paths (@module), use and replace file paths
; (@string.special), escape sequences and brackets.
;
; Known gap: the grammar knows only go, use and replace. A toolchain (Go 1.21)
; or godebug (Go 1.23) line parses as an ERROR and goes uncoloured; the rest
; of the file still colours.

[
  "replace"
  "go"
  "use"
] @keyword

"=>" @operator

(comment) @comment

[
  (version)
  (go_version)
] @number

(module_path) @module

(file_path) @string.special

(escape_sequence) @escape

[
  "("
  ")"
] @punctuation.bracket
