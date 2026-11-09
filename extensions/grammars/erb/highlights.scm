; From tree-sitter-embedded-template at 3499d85f0a0d937c507a4a65368f2f63772786e1,
; https://github.com/tree-sitter/tree-sitter-embedded-template, path
; queries/highlights.scm, license MIT. Unchanged: it colours only the
; directives' delimiters and comment directives; the HTML and the code inside
; the directives take their colours from injections.scm.

(comment_directive) @comment

[
  "<%#"
  "<%"
  "<%="
  "<%_"
  "<%-"
  "%>"
  "-%>"
  "_%>"
] @keyword
