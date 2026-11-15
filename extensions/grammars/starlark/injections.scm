; From tree-sitter-starlark's queries/injections.scm at
; a453dbf3ba433db0e5ec621a38a7e59d72e4dc69. The regex injection into strings
; passed to `re.*` is restored, now that the built-in regex grammar exists, as
; upstream wrote it but for the capture: upstream names it @regex, which this
; engine doesn't read, so it is @injection.content with the language set, on the string's
; content (a string's named children are excluded from a capture of the whole
; string), and upstream's "^r.*" test is made on its start. The
; comment injection stays dropped, as the editor has no comment language.
((call
  function: (attribute
    object: (identifier) @_re)
  arguments: (argument_list
    (string
      (string_start) @_start
      (string_content) @injection.content)))
 (#eq? @_re "re")
 (#match? @_start "^r.*")
 (#set! injection.language "regex"))
