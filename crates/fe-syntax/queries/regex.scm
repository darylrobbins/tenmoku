; Ours. After tree-sitter-regex's highlights.scm, so these win on the same
; node. Its captures fit our buckets except where a regex reads differently
; from the escapes and constants it calls them: assertions match a position,
; not a character; its literal text is the host's; a group's name
; labels it; a backreference names a group's text; `{n,m}` is a quantifier.

[
  (start_assertion)
  (end_assertion)
  (boundary_assertion)
  (non_boundary_assertion)
] @keyword

; Literal characters keep the host's colour for a regex literal, and stay out
; of spell checking with it, as a `string` would not.
[
  (pattern_character)
  (class_character)
  (posix_class_name)
] @string.special

(group_name) @label

[
  (decimal_escape)
  (backreference_escape)
  (named_group_backreference)
] @variable.builtin

; The crate's query makes these brackets wherever they are.
(backreference_escape ">" @variable.builtin)
(named_group_backreference [ "(?P=" ")" ] @variable.builtin)

(count_quantifier
  [
    "{"
    "}"
    (decimal_digits)
    ","
  ] @operator)

[
  (any_character)
  (lazy)
] @operator
