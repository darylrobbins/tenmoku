; Ours. After the crate's query, so these win on the same node. A name written
; as a key in a table constructor, `{ title = x }`, is a key, Lapis like a data
; file's. Field access and bracketed keys (`["k"] = v`, `[k] = v`) keep their
; highlights.

(field
  name: (identifier) @string.special.key)

; A bracketed key is an expression, though the grammar names it the same way:
; it keeps the crate's highlight.
(field
  "["
  name: (identifier) @field)

; The crate's: a key whose value is a function stays a function.
(table_constructor
  (field
    name: (identifier) @function
    value: (function_definition)))
