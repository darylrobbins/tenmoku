;; definitions query, ours, written from tree-sitter-ruby 0.23.1's
;; node-types.json; capture convention of tree-sitter tags
; Follows outline/ruby.scm in the symbols query.

; attr_accessor :name, :total: each symbol is a field, named without its
; colon. Ruby declares no other fields, and its variables are locals.
((call
  !receiver
  method: (identifier) @_attr
  arguments: (argument_list (simple_symbol) @name @definition.field))
  (#match? @_attr "^attr_(reader|writer|accessor)$"))
