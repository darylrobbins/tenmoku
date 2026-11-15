; Used in place of tree-sitter-php's injections.scm for PHP.
; From tree-sitter-php 0.24.2, https://github.com/tree-sitter/tree-sitter-php,
; path queries/injections.scm, license MIT. Changes: heredocs and nowdocs
; capture their text pieces, combined, rather than the whole body.
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
;
; Regular expressions: the first argument of preg_match, preg_match_all,
; preg_replace, preg_replace_callback, preg_split and preg_grep, when a plain
; string, is injected as `regex` without its delimiters and flags
; (`'/a+b/i'` injects `a+b`), through `injection.trim-delimiters`, our own
; property. Limit: an escaped PHP string (single or double quoted) is injected
; only when its content has no backslash, as only then does the host text equal
; the regex text; `'\\d'` is two source characters. A double-quoted string
; with an interpolation stays plain.

((comment) @injection.content
  (#set! injection.language "phpdoc"))

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

(function_call_expression
  function: (name) @_fn
  arguments: (arguments
    .
    (argument
      [
        (string . (string_content) @injection.content .)
        (encapsed_string . (string_content) @injection.content .)
      ]))
  (#any-of? @_fn "preg_match" "preg_match_all" "preg_replace" "preg_replace_callback" "preg_split" "preg_grep")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex")
  (#set! injection.trim-delimiters))
