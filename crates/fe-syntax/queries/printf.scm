; Ours. tree-sitter-printf's own highlights paint flags and widths in unrelated
; colours; a whole directive is one placeholder here, and `%%` is an escape.
; A directive containing whitespace is prose the C grammar swallowed (`%w h`
; for "%w happened"), so it stays plain; see the spec's Known limit.
((format) @string.placeholder
  (#not-match? @string.placeholder "\\s"))
"%%" @escape
