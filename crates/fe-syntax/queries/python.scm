; Ours. Layered after the crate's own query: f-string placeholders get VS Code's
; placeholder colour; the expression inside keeps its normal colours.

; The upstream query paints a whole interpolation `embedded`; clear that (a
; narrower node wins, so the expression's own captures survive). An
; interpolation nested in a format spec is a `format_expression` node.
[(interpolation) (format_expression)] @none

(interpolation
  "{" @string.placeholder
  "}" @string.placeholder)
(format_expression
  "{" @string.placeholder
  "}" @string.placeholder)

(interpolation (type_conversion) @string.placeholder)
(format_expression (type_conversion) @string.placeholder)
(interpolation (format_specifier) @string.placeholder)
(format_expression (format_specifier) @string.placeholder)

(escape_interpolation) @escape

; A name that labels a value is a property: a call's keyword argument. A
; definition's `default_parameter` keeps its parameter highlight.
(keyword_argument name: (identifier) @property)
