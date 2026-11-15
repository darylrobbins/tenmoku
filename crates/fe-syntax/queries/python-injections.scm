; Ours. Inject the hidden format layers into Python's two non-f-string
; formatting forms, so their placeholders colour apart from the string. An
; f-string never injects: python.scm colours its fields, and a layer would
; paint its interpolations twice. A variable on the left of `%`, or as
; `.format`'s receiver, injects nothing. Each string_content piece injects
; alone, so a directive an escape sequence splits paints nothing.
;
; The regex grammar is injected into the first argument of re.compile, search,
; match, fullmatch, sub, subn, split, findall and finditer (plain `re.` only,
; no aliases). Limit: raw strings (r"..", rb"..") are injected whole; an escaped
; (non-raw) string only when its content has no backslash, as only then does
; the host text equal the regex text. With a backslash it stays a plain string.

; "%s: %d" % args, b"%s" % x
(binary_operator
  left: (string
    (string_start) @_start
    (string_content) @injection.content)
  operator: "%"
  (#not-match? @_start "[fF]")
  (#set! injection.language "format-py-percent"))

; "{} {0!r:>10}".format(...)
(call
  function: (attribute
    object: (string
      (string_start) @_start
      (string_content) @injection.content)
    attribute: (identifier) @_method)
  (#eq? @_method "format")
  (#not-match? @_start "[fF]")
  (#set! injection.language "format-brace"))

; Logging style: log.info("%s took %d", a, b). The receiver's type is
; unknown, so any receiver does; the only false positive is a "%d" first
; argument to a method of one of these names.
(call
  function: (attribute
    attribute: (identifier) @_method)
  arguments: (argument_list
    .
    (string
      (string_start) @_start
      (string_content) @injection.content))
  (#any-of? @_method "debug" "info" "warning" "warn" "error" "exception" "critical")
  (#not-match? @_start "[fF]")
  (#set! injection.language "format-py-percent"))

; log.log(level, "%s", x)
(call
  function: (attribute
    attribute: (identifier) @_method)
  arguments: (argument_list
    .
    (_)
    .
    (string
      (string_start) @_start
      (string_content) @injection.content))
  (#eq? @_method "log")
  (#not-match? @_start "[fF]")
  (#set! injection.language "format-py-percent"))

; re.compile(r"\d+"): a raw string, whole
(call
  function: (attribute
    object: (identifier) @_mod
    attribute: (identifier) @_fn)
  arguments: (argument_list
    .
    (string
      (string_start) @_start
      (string_content) @injection.content))
  (#eq? @_mod "re")
  (#any-of? @_fn "compile" "search" "match" "fullmatch" "sub" "subn" "split" "findall" "finditer")
  (#match? @_start "^[bB]?[rR]")
  (#not-match? @_start "[fF]")
  (#set! injection.language "regex"))

; re.search("a+b", s): an escaped string, only when it holds no backslash
(call
  function: (attribute
    object: (identifier) @_mod
    attribute: (identifier) @_fn)
  arguments: (argument_list
    .
    (string
      (string_start) @_start
      .
      (string_content) @injection.content
      .
      (string_end)))
  (#eq? @_mod "re")
  (#any-of? @_fn "compile" "search" "match" "fullmatch" "sub" "subn" "split" "findall" "finditer")
  (#not-match? @_start "[rRfF]")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))
