; Ours. The idea (inject the printf grammar into format strings) is from
; nvim-treesitter's queries/go/injections.scm, Apache-2.0; the patterns are
; written afresh.
; The format is the first string argument of a Printf-style call, or the one
; after the writer, input or buffer argument for Fprintf, Fscanf, Sscanf
; and Appendf. The two
; patterns take disjoint name sets, so a value argument never injects.
(call_expression
  function: [
    (identifier) @_fn
    (selector_expression field: (field_identifier) @_fn)
  ]
  arguments: (argument_list
    . [
      (interpreted_string_literal (interpreted_string_literal_content) @injection.content)
      (raw_string_literal (raw_string_literal_content) @injection.content)
    ])
  (#match? @_fn "^[A-Za-z]+f$")
  (#not-any-of? @_fn "Fprintf" "Fscanf" "Sscanf" "Appendf")
  (#set! injection.language "printf"))

(call_expression
  function: [
    (identifier) @_fn
    (selector_expression field: (field_identifier) @_fn)
  ]
  arguments: (argument_list
    .
    (_)
    .
    [
      (interpreted_string_literal (interpreted_string_literal_content) @injection.content)
      (raw_string_literal (raw_string_literal_content) @injection.content)
    ])
  (#any-of? @_fn "Fprintf" "Fscanf" "Sscanf" "Appendf")
  (#set! injection.language "printf"))
