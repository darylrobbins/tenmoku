; From tree-sitter-elixir's queries/injections.scm at commit
; cb32ccbe17c67037b09e31bf9beb22cdae6ed8e9 (Apache-2.0, see LICENSE).
; Changes: the heex and vue injections are dropped, as those languages don't
; exist here. The ~r/~R regex sigil injection is restored, now that the
; built-in regex grammar exists.

; Regex
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#any-of? @_sigil_name "r" "R")
 (#set! injection.language "regex")
 (#set! injection.combined))

; SQL injection
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#eq? @_sigil_name "SQL")
 (#set! injection.language "sql")
 (#set! injection.combined))

; Markdown
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#eq? @_sigil_name "MD")
 (#set! injection.language "markdown")
 (#set! injection.combined))

; Python
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#eq? @_sigil_name "PY")
 (#set! injection.language "python")
 (#set! injection.combined))

; Bash
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#eq? @_sigil_name "BASH")
 (#set! injection.language "bash")
 (#set! injection.combined))

; JavaScript
((sigil
  (sigil_name) @_sigil_name
  (quoted_content) @injection.content)
 (#eq? @_sigil_name "JS")
 (#set! injection.language "javascript")
 (#set! injection.combined))
