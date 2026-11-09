; From tree-sitter-twig, https://github.com/gbprod/tree-sitter-twig, commit
; 2208d2a3c3ee7ef378e97df2e51c18feb7ee9dfc, path queries/injections.scm,
; license MIT (text in LICENSE-TWIG). Unchanged: the text between
; directives is HTML, combined. A block's body text is that block's own
; child, so each block body is its own HTML layer.
;
; Upstream tree-sitter-jinja's own queries/injections.scm (Apache-2.0, at
; f46e979278123e3b6949381896f3917f378af068) only injects line statements
; into `jinja_inline`, the repository's second grammar, which isn't shipped
; here, so it isn't used.
((content) @injection.content
 (#set! injection.language "html")
 (#set! injection.combined))
