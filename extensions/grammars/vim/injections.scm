; From tree-sitter-grammars/tree-sitter-vim queries/vim/injections.scm at
; 039c8d0aa1deae00ddeb0374dd70bcc0ec56938d, licence MIT (see LICENSE). Only the
; injections into languages that are always available are kept (Lua, Python):
; Ruby is feature-gated, and the regex, comment and self-injections and the
; commented-out Perl one have no language to inject.
(lua_statement
  (script
    (body) @injection.content
    (#set! injection.language "lua")))

(lua_statement
  (chunk) @injection.content
  (#set! injection.language "lua"))

(python_statement
  (script
    (body) @injection.content
    (#set! injection.language "python")))

(python_statement
  (chunk) @injection.content
  (#set! injection.language "python"))
