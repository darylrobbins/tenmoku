; Indent query for Go, read by `indent.rs`.
; Adapted from nvim-treesitter, https://github.com/nvim-treesitter/nvim-treesitter
; commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6,
; path runtime/queries/go/indents.scm, license Apache-2.0.
; Changes: `indent.rs` indents between brackets itself, so the switch
; statements are captured `@indent.dedent`, which keeps their braces from
; indenting the cases, and cases begin immediately, as their bodies have no
; closer. Go's `else` follows its `}` on the same row, so it needs no
; branch.

[
  (import_declaration)
  (const_declaration)
  (var_declaration)
  (type_declaration)
  (func_literal)
  (literal_value)
  (block)
  (call_expression)
  (parameter_list)
  (field_declaration_list)
  (interface_type)
] @indent.begin

([
  (expression_case)
  (communication_case)
  (type_case)
  (default_case)
] @indent.begin
  (#set! indent.immediate))

; gofmt puts a switch's cases at the switch's own level.
[
  (expression_switch_statement)
  (type_switch_statement)
  (select_statement)
] @indent.dedent

(literal_value
  "}" @indent.branch)

(block
  "}" @indent.branch)

(field_declaration_list
  "}" @indent.branch)

(interface_type
  "}" @indent.branch)

(const_declaration
  ")" @indent.branch)

(import_spec_list
  ")" @indent.branch)

(var_spec_list
  ")" @indent.branch)

[
  "}"
  ")"
] @indent.end

(parameter_list
  ")" @indent.branch)

(comment) @indent.ignore
