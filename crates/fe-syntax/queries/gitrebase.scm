; gitrebase highlights, ours; goes after tree-sitter-git-rebase's highlights.scm
; for the forms it leaves out or misreads. Later patterns win.

; `update-ref <ref>`
((operation
  (command) @function
  (label) @constant)
 (#match? @function "^(u|update-ref)$"))

((command) @keyword
 (#eq? @keyword "noop"))

; What `exec` runs.
((operation
  (command) @_command
  (message) @string)
 (#match? @_command "^(x|exec)$"))

; `merge [-C <hash> | -c <hash>] <label> [<subject>]`: upstream only knows the
; form with an option and a subject.
((operation
  (command) @keyword
  (label) @constant)
 (#match? @keyword "^(m|merge)$"))

((operation
  (command) @_command
  (message) @string)
 (#match? @_command "^(m|merge)$"))

((operation
  (command) @_command
  (option) .
  (label) @constant.builtin)
 (#match? @_command "^(m|merge)$"))

; `fixup -C <hash> <subject>` parses the subject's first word as a second label.
((operation
  (command) @_command
  (option)
  (label)
  (label) @comment)
 (#match? @_command "^(f|fixup)$"))
