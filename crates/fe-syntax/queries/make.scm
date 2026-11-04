; Ours. Makefile highlights written for tree-sitter-make 1.1.1's node types;
; replaces the crate's minimal highlights query.

(comment) @comment

; Rules: targets are functions, prerequisites a distinct colour.
(rule (targets (word) @function))
(rule (targets (_ (word) @function)))
(rule (prerequisites (word) @variable.parameter))
(rule (prerequisites (_ (word) @variable.parameter)))

; Special targets win over plain ones (later patterns win).
(targets
  (word) @keyword
  (#match? @keyword "^\\.(PHONY|SUFFIXES|DEFAULT|PRECIOUS|INTERMEDIATE|SECONDARY|SECONDEXPANSION|DELETE_ON_ERROR|IGNORE|LOW_RESOLUTION_TIME|SILENT|EXPORT_ALL_VARIABLES|NOTPARALLEL|ONESHELL|POSIX)$"))

; Variables
(variable_assignment name: (word) @variable)
(shell_assignment name: (word) @variable)
(define_directive name: (word) @variable)
(ifdef_directive variable: (word) @variable)
(ifndef_directive variable: (word) @variable)
(undefine_directive variable: (word) @variable)
(variable_reference (word) @variable)
(substitution_reference (word) @variable)

(variable_assignment name: (word) @variable.builtin
  (#match? @variable.builtin "^(VPATH|MAKEFILE_LIST|MAKE_RESTARTS|\\.DEFAULT_GOAL|\\.RECIPEPREFIX|\\.EXTRA_PREREQS)$"))
(variable_reference (word) @variable.builtin
  (#match? @variable.builtin "^(MAKE|MAKEFLAGS|MAKELEVEL|MAKEFILE_LIST|CURDIR|SHELL|VPATH|\\.VARIABLES|\\.FEATURES|\\.INCLUDE_DIRS)$"))

[ "VPATH" ".RECIPEPREFIX" ] @variable.builtin

(automatic_variable) @variable.builtin

[ "=" ":=" "::=" "?=" "+=" "!=" ] @operator

; `$(`, `${`, `)` and `}` around references and calls.
(variable_reference [ "$" "(" ")" "{" "}" ] @punctuation.special)
(substitution_reference [ "$" "(" ")" "{" "}" ] @punctuation.special)
(function_call [ "$" "(" ")" "{" "}" ] @punctuation.special)
(shell_function [ "$" "(" ")" "{" "}" ] @punctuation.special)
(automatic_variable "$" @punctuation.special)
(automatic_variable [ "@" "%" "<" "?" "^" "+" "*" "D" "F" "/" ] @variable.builtin)

(substitution_reference ":" @punctuation.delimiter)
(escape) @string.escape
"$$" @string.escape

; Functions
(function_call function: _ @function)
(shell_function function: _ @function)
(function_call function: [ "if" "or" "and" "foreach" ] @keyword)

(function_call
  function: "error"
  (arguments (text) @comment))

; Directives
[
  "ifeq" "ifneq" "ifdef" "ifndef" "else" "endif"
  "define" "endef" "vpath" "undefine"
  "export" "unexport" "override" "private"
  "include" "sinclude" "-include"
] @keyword

; Recipe prefixes
(recipe_line [ "@" "-" "+" ] @operator)

[ ":" "&:" "::" "|" ";" "," ] @punctuation.delimiter

(string) @string
