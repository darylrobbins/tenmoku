; Adapted from tree-sitter-gleam's queries/injections.scm at commit
; fa6d0d94804f1b342e175865a3dbeb316ee5115e (Apache-2.0, see LICENSE).
((doc_comment_content) @injection.content
 (#set! injection.language "markdown")
 (#set! injection.combined))
