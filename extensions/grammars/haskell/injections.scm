; From tree-sitter-haskell's queries/injections.scm at
; 97288e585b0bd44199720280d067fa35bacd81ff, under its MIT licence (LICENSE).
; Changed: only the quasi-quoters naming a language the app has are kept;
; the injections into "comment" and "haskell_persistent", and the one taking
; any quoter's name as a language, are dropped.

; -----------------------------------------------------------------------------
; shakespeare library
; NOTE: doesn't support templating
; CSS: Text.Cassius, Text.Lucius
(quasiquote
  (quoter) @_name
  (#any-of? @_name "cassius" "lucius")
  (quasiquote_body) @injection.content
  (#set! injection.language "css"))

; HTML: Text.Hamlet
(quasiquote
  (quoter) @_name
  (#any-of? @_name "shamlet" "xshamlet" "hamlet" "xhamlet" "ihamlet")
  (quasiquote_body) @injection.content
  (#set! injection.language "html"))

; JS: Text.Julius
(quasiquote
  (quoter) @_name
  (#any-of? @_name "js" "julius")
  (quasiquote_body) @injection.content
  (#set! injection.language "javascript"))

; TS: Text.TypeScript
(quasiquote
  (quoter) @_name
  (#any-of? @_name "tsc" "tscJSX")
  (quasiquote_body) @injection.content
  (#set! injection.language "typescript"))

; -----------------------------------------------------------------------------
; HSX
(quasiquote
  (quoter) @_name
  (#eq? @_name "hsx")
  (quasiquote_body) @injection.content
  (#set! injection.language "html"))

; -----------------------------------------------------------------------------
; Inline JSON from aeson
(quasiquote
  (quoter) @_name
  (#eq? @_name "aesonQQ")
  (quasiquote_body) @injection.content
  (#set! injection.language "json"))

; -----------------------------------------------------------------------------
; SQL
; postgresql-simple
(quasiquote
  (quoter) @_name
  (#eq? @_name "sql")
  (quasiquote_body) @injection.content
  (#set! injection.language "sql"))
