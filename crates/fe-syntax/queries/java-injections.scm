; Ours. The regex grammar is injected into the string-literal first argument of
; Pattern.compile(..) and Pattern.matches(regex, input), and of any method named
; matches, replaceAll, replaceFirst or split (String's). The receiver's type is
; not knowable, so any receiver is accepted: a user method of one of those names
; taking a string first also injects (an accepted over-match).
; Limit: Java has no raw or verbatim string except the text block
; ("""..."""). An escaped string and a text block are injected only when their
; content has no backslash, as only then does the host text equal the regex
; text; `\\d` is two source characters. That leaves most Java regexes plain
; strings, which is the chosen limit.

; Pattern.compile("a+b")
(method_invocation
  object: (identifier) @_cls
  name: (identifier) @_fn
  arguments: (argument_list
    .
    (string_literal . (string_fragment) @injection.content .))
  (#eq? @_cls "Pattern")
  (#any-of? @_fn "compile" "matches")
  (#set! injection.language "regex"))

; "x".matches("a+b"), s.split(",")
(method_invocation
  name: (identifier) @_fn
  arguments: (argument_list
    .
    (string_literal . (string_fragment) @injection.content .))
  (#any-of? @_fn "matches" "replaceAll" "replaceFirst" "split")
  (#set! injection.language "regex"))

; text blocks: Pattern.compile("""...""")
(method_invocation
  object: (identifier) @_cls
  name: (identifier) @_fn
  arguments: (argument_list
    .
    (string_literal (multiline_string_fragment) @injection.content))
  (#eq? @_cls "Pattern")
  (#any-of? @_fn "compile" "matches")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))

(method_invocation
  name: (identifier) @_fn
  arguments: (argument_list
    .
    (string_literal (multiline_string_fragment) @injection.content))
  (#any-of? @_fn "matches" "replaceAll" "replaceFirst" "split")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))
