; php/'s injections.scm, used in place of tree-sitter-php's injections.scm.
; From tree-sitter-php 0.24.2, https://github.com/tree-sitter/tree-sitter-php,
; path queries/injections.scm, license MIT. Changes: heredocs and nowdocs
; capture their text pieces, combined, rather than the whole body (as php/'s
; does), and the injection of comments as `phpdoc`, a language there is no
; grammar for, is dropped.
;
; The crate captures `heredoc_body` and `nowdoc_body`, whose text all sits in
; named children (`string_content`, `nowdoc_string`). An injection leaves its
; content's named children out, so only the gaps between them, such as
; newlines and the braces of `{$x}`, were injected. Capturing the pieces and
; combining them parses the text as one layer around the interpolations,
; which stay PHP.
;
; A heredoc's scanner skips the line break ending each line without making a
; node of it, so `injection.include-gaps`, our own property, adds the body's
; bytes that no child covers. Without them the lines run together, and an
; SQL `--` comment swallows the next line.

(heredoc
  (heredoc_body
    (string_content) @injection.content)
  (heredoc_end) @injection.language
  (#set! injection.combined)
  (#set! injection.include-gaps))

(nowdoc
  (nowdoc_body
    (nowdoc_string) @injection.content)
  (heredoc_end) @injection.language
  (#set! injection.combined)
  (#set! injection.include-gaps))
