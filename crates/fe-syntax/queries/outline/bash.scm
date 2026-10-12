;; outline query, ours; capture convention of tree-sitter tags

; Both forms, f() { ... } and function f { ... }, with or without the
; parentheses. One in another's body nests under it.
(function_definition name: (word) @name) @definition.function
