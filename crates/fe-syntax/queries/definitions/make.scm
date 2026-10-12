;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/make.scm in the symbols query.

; A variable's assignment, at the top or in a conditional, which isn't a
; scope, and with or without override, export or private. A target's own,
; tool: CFLAGS += -g, isn't listed.
(variable_assignment
  !target_or_pattern
  name: (word) @name) @definition.variable
