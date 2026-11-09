; From tree-sitter-embedded-template at 3499d85f0a0d937c507a4a65368f2f63772786e1,
; https://github.com/tree-sitter/tree-sitter-embedded-template, path
; queries/injections-ejs.scm, license MIT. Unchanged: the text between the
; directives is HTML and the code inside them JavaScript.

((content) @injection.content
 (#set! injection.language "html")
 (#set! injection.combined))

((code) @injection.content
 (#set! injection.language "javascript")
 (#set! injection.combined))
