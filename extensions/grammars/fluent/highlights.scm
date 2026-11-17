; Written for Tenmoku against mozsearch/tree-sitter-fluent at
; e4ff53883eb6eddc832a73452fdc3d9c46fe698d, which ships no queries; nothing
; is copied. Last pattern wins.
;
; A message's text is @string, as a .properties value is, so that its
; placeables stand out. Junk (what doesn't parse) is an ERROR node, which
; stays plain: fe-syntax's vocabulary has no error colour.

; Comments: "#" (an entry's or standalone), "##" (a group's) and "###" (the
; resource's).
(comment) @comment
(group_comment) @comment.documentation
(resource_comment) @comment.documentation

; Text and literals.
(text) @string
(string_literal) @string
(number_literal) @number

; Entries: messages, terms (whose identifier has its "-") and attributes.
(message id: (identifier) @property)
(term id: (term_identifier) @constant)
(attribute id: (identifier) @attribute)
(attribute "." @punctuation.delimiter)

(message "=" @operator)
(term "=" @operator)
(attribute "=" @operator)

; Placeables.
(placeable [ "{" "}" ] @punctuation.special)

; References: variables ($var), messages and terms, with their attributes.
(variable_reference "$" @punctuation.special)
(variable_reference id: (identifier) @variable.parameter)
(message_reference id: (identifier) @property)
(term_reference id: (term_identifier) @constant)
(message_reference "." @punctuation.delimiter)
(term_reference "." @punctuation.delimiter)
(message_reference attribute: (identifier) @attribute)
(term_reference attribute: (identifier) @attribute)

; Function calls (NUMBER(...), DATETIME(...)) and call arguments, which a
; term reference may have too.
(function_reference id: (identifier) @function.builtin)
(call_arguments [ "(" ")" ] @punctuation.bracket)
(call_arguments "," @punctuation.delimiter)
(named_argument name: (identifier) @variable.parameter)
(named_argument ":" @punctuation.delimiter)

; Select expressions: the selector's "->", the variants' keys and the
; default's "*".
(select_expression "->" @operator)
(variant_key [ "[" "]" ] @punctuation.bracket)
(variant_key (identifier) @string.special.key)
(variant default: "*" @punctuation.special)
