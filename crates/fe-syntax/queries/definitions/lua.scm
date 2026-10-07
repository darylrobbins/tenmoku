;; definitions query, ours, written from tree-sitter-lua 0.5.0's
;; node-types.json; capture convention of tree-sitter tags
; Follows outline/lua.scm in the symbols query.

;; A list of one target is matched with anchors, `. name: ... .`, rather
;; than by a predicate: a long list's targets would each start a match the
;; query engine keeps until the list ends. #not-sole? takes the target and
;; its list, captured: a node's parent costs its siblings.

; The fields of a module's table other than functions, which the outline
; query lists: local Account = { balance = 0 }.
(chunk
  (variable_declaration
    (assignment_statement
      (variable_list . name: (identifier) .)
      (expression_list
        .
        value: (table_constructor
          (field name: (identifier) @name) @definition.field) .))))

(chunk
  (assignment_statement
    (variable_list . name: (identifier) .)
    (expression_list
      .
      value: (table_constructor
        (field name: (identifier) @name) @definition.field) .)))

; local x = 1, local x and counter = 0 at the top of the file. With one
; target the item is the statement, which a module or function in the
; outline query wins over; with several, as local a, b = 1, 2, each name
; is its own item, which the outline's function wins over when the name is
; assigned one. A local in a function or block isn't listed, nor is an
; assignment to a field, as M.version = "1.0".
(chunk
  (variable_declaration
    (assignment_statement
      (variable_list . name: (identifier) @name .)) @definition.variable))

(chunk
  (variable_declaration
    (variable_list . name: (identifier) @name .) @definition.variable))

(chunk
  (assignment_statement
    (variable_list . name: (identifier) @name .)) @definition.variable)

((chunk
  (variable_declaration
    (assignment_statement
      (variable_list name: (identifier) @name @definition.variable) @_list)))
  (#not-sole? @name @_list))

((chunk
  (variable_declaration
    (variable_list name: (identifier) @name @definition.variable) @_list))
  (#not-sole? @name @_list))

((chunk
  (assignment_statement
    (variable_list name: (identifier) @name @definition.variable) @_list))
  (#not-sole? @name @_list))
