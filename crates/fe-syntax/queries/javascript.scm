; Ours. After the crates' queries for JavaScript, TypeScript and TSX, so these
; win on the same node. An identifier written as a key in an object literal or
; a destructuring pattern is a `property`: a key written in code labels a value; data files
; keep `string.special.key`. Member access,
; shorthand, quoted, numeric and computed keys, interface and type-literal
; members, class fields and JSX attributes keep their highlights.

(pair
  key: (property_identifier) @property)

(pair_pattern
  key: (property_identifier) @property)

; The crate's: a key whose value is a function stays a method.
(pair
  key: (property_identifier) @function.method
  value: [(function_expression) (arrow_function)])
