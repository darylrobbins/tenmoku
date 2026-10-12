;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/bash.scm in the symbols query.

; A top-level assignment, plain, several on a line or declared: X=1,
; A=1 B=2, export X=1, readonly X=1. An array element's, arr[0]=1, is the
; array's. One in a function, a loop or a command's prefix, X=1 cmd, isn't
; listed.
(program
  (variable_assignment
    name: [
      (variable_name) @name
      (subscript name: (variable_name) @name)
    ]) @definition.variable)

(program
  (variable_assignments
    (variable_assignment
      name: [
        (variable_name) @name
        (subscript name: (variable_name) @name)
      ]) @definition.variable))

(program
  (declaration_command
    (variable_assignment
      name: [
        (variable_name) @name
        (subscript name: (variable_name) @name)
      ]) @definition.variable))
