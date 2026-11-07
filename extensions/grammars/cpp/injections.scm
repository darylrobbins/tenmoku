; Ours. Inject the printf-c layer into the format string of printf-style calls.
; Each pattern matches one argument position with `.` anchors and a name set,
; so a value argument never injects. In a concatenated string each piece
; injects alone: a verb split across pieces, or by a PRId64-style macro,
; paints nothing.
; The std::-qualified forms are matched too.
(call_expression
  function: [(identifier) @_fn (qualified_identifier scope: (namespace_identifier) @_ns name: (identifier) @_fn)]
  arguments: (argument_list
    .
    [
      (string_literal (string_content) @injection.content)
      (concatenated_string (string_literal (string_content) @injection.content))
      (raw_string_literal (raw_string_content) @injection.content)
    ])
  (#any-of? @_fn "printf" "vprintf")
  (#eq? @_ns "std")
  (#set! injection.language "printf-c"))

(call_expression
  function: [(identifier) @_fn (qualified_identifier scope: (namespace_identifier) @_ns name: (identifier) @_fn)]
  arguments: (argument_list
    .
    (_)
    .
    [
      (string_literal (string_content) @injection.content)
      (concatenated_string (string_literal (string_content) @injection.content))
      (raw_string_literal (raw_string_content) @injection.content)
    ])
  (#any-of? @_fn "fprintf" "vfprintf" "dprintf" "sprintf" "vsprintf" "asprintf" "vasprintf" "syslog")
  (#eq? @_ns "std")
  (#set! injection.language "printf-c"))

(call_expression
  function: [(identifier) @_fn (qualified_identifier scope: (namespace_identifier) @_ns name: (identifier) @_fn)]
  arguments: (argument_list
    .
    (_)
    .
    (_)
    .
    [
      (string_literal (string_content) @injection.content)
      (concatenated_string (string_literal (string_content) @injection.content))
      (raw_string_literal (raw_string_content) @injection.content)
    ])
  (#any-of? @_fn "snprintf" "vsnprintf")
  (#eq? @_ns "std")
  (#set! injection.language "printf-c"))
