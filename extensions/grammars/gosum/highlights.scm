; From tree-sitter-go-sum at 27816eb6b7315746ae9fcf711e4e1396dc1cf237,
; https://github.com/tree-sitter-grammars/tree-sitter-go-sum, path
; queries/highlights.scm, license MIT. Changes: captures in fe-syntax's
; vocabulary, one each: module paths are @module (upstream's @string
; @text.uri); a version, with its pre-release word, pseudo-version numbers,
; hex and +incompatible, is one @number (upstream colours its parts apart);
; the /go.mod suffix's name is @string.special; hashes are @string (upstream's
; @symbol); only `:` and `/` are delimiters, as a version's `.` and `-` are
; part of it.

(module_path) @module

(version) @number

(checksum
  "go.mod" @string.special)

(hash_version) @attribute

(hash) @string

[
  ":"
  "/"
] @punctuation.delimiter
