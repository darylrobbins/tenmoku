; Ours. After bash's query, which leaves unquoted text plain. In a dotenv file
; that text is the value, so it reads like a quoted one, around any `${VAR}` too.
(variable_assignment
  value: [
    (word)
    (number)
  ] @string)

(variable_assignment
  value: (concatenation
    [
      (word)
      (number)
    ] @string))
