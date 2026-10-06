;; outline query, adapted from tree-sitter-lua 0.5.0 queries/tags.scm (MIT);
;; capture convention of tree-sitter tags
; Changed: only @definition.* and @name are kept (no references); a
; function taking self is a method, as function M:f() is; a table assigned
; to one name at the top of the file is a module, and the functions in its
; constructor are its functions and methods.

; function M:describe(), which takes self.
(function_declaration
  name: (method_index_expression method: (identifier) @name)) @definition.method

; function M.f(self) and f = function(self), written out.
((function_declaration
  name: [
    (identifier) @name
    (dot_index_expression field: (identifier) @name)
  ]
  parameters: (parameters . name: (identifier) @_self)) @definition.method
  (#eq? @_self "self"))

((field
  name: (identifier) @name
  value: (function_definition
    parameters: (parameters . name: (identifier) @_self))) @definition.method
  (#eq? @_self "self"))

(function_declaration
  name: [
    (identifier) @name
    (dot_index_expression field: (identifier) @name)
  ]) @definition.function

; M.callback = function(event) ... end, local f = function() ... end: one
; target and one value. In local a, b = function() end, 1, a and b are
; variables, as the definitions query lists them.
((assignment_statement
  (variable_list
    name: [
      (identifier) @name
      (dot_index_expression field: (identifier) @name)
    ])
  (expression_list
    .
    value: (function_definition) .)) @definition.function
  (#sole? @name))

(field
  name: (identifier) @name
  value: (function_definition)) @definition.function

; local M = {} or M = { ... } at the top of the file: Lua's modules and
; classes are tables. The item is the assignment, as for a variable of one
; target, which this pattern wins over.
((chunk
  (variable_declaration
    (assignment_statement
      (variable_list name: (identifier) @name)
      (expression_list . value: (table_constructor) .)) @definition.module))
  (#sole? @name))

((chunk
  (assignment_statement
    (variable_list name: (identifier) @name)
    (expression_list . value: (table_constructor) .)) @definition.module)
  (#sole? @name))
