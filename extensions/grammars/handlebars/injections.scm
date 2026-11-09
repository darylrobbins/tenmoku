; Ours, for tree-sitter-handlebars at 6cf6821144efca0ba9327497f0523f5ebc04abef
; (upstream ships no injections.scm): the grammar's `text` node is exactly the
; non-template content, which is HTML.
((text) @injection.content
 (#set! injection.language "html")
 (#set! injection.combined))
