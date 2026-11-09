; Ours. After the crates' queries for JavaScript, TypeScript and TSX, so these
; win on the same node. An identifier written as a key in an object literal or
; a destructuring pattern is a key, Lapis like a data file's. Member access,
; shorthand, quoted, numeric and computed keys, interface and type-literal
; members, class fields and JSX attributes keep their highlights.

(pair
  key: (property_identifier) @string.special.key)

(pair_pattern
  key: (property_identifier) @string.special.key)

; The crate's: a key whose value is a function stays a method.
(pair
  key: (property_identifier) @function.method
  value: [(function_expression) (arrow_function)])
