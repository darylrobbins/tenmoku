; Ours. Go's query has earlier patterns win, so this file goes before the
; crate's, and in it the exception before the rule.
;
; A key in a composite literal is an expression. In a struct literal it's a
; field name, a key, Lapis like a data file's; in a map, slice or array
; literal it's a value, a variable or constant. The type-elided inner literals
; of `[]Book{{Title: x}}` have no type, and count as structs. A named map type
; (`type M map[string]int; M{k: 1}`) reads as a struct too: the grammar can't
; tell it apart.

(composite_literal
  type: [(map_type) (slice_type) (array_type) (implicit_length_array_type)]
  body: (literal_value
    (keyed_element
      key: (literal_element (identifier) @variable))))

(keyed_element
  key: (literal_element (identifier) @string.special.key))
