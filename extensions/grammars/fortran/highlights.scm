; Adapted from https://github.com/stadelmanma/tree-sitter-fortran
; queries/highlights.scm at 72f8759dea28ff6671e0868a9ed5ad4a18413917 (MIT).
; Later patterns win, as upstream's order assumes.
; Changed: `(custom_directive)` is @keyword (upstream's @custom_directive
; has no colour here); the `module`, `submodule` and `type` statements are
; @keyword, not @module and @type, and the names of programs, modules and
; submodules (and `(module_name)`) are @module, not @variable; block labels,
; and the label of `exit`/`cycle`, are @label, not @attribute; a `block`
; construct colours its `block` keyword, not the whole construct.
; Added: the operators `**`, `//`, `=>` and `.not.`; the keywords of the
; read, close, inquire, nullify, format, stop and where statements and of
; `non_intrinsic`; generic interface names, type-bound procedure bindings and
; the procedures a `procedure` statement names, as @function.

(identifier) @variable

(module_name) @module

(string_literal) @string
(number_literal) @number
(statement_label) @number
(statement_label_reference) @number
(boolean_literal) @boolean
(comment) @comment
(custom_directive) @keyword

[
 (derived_type)
 (import_statement)
 (intrinsic_type)
 (type_name)
] @type

(intrinsic_type) @type.builtin

(base_type_specifier
  (identifier) @type)


[
 (abstract_specifier)
 (access_specifier)
 (none)
 (procedure_attributes)
 (procedure_qualifier)
 (type_qualifier)
] @attribute

[
 (block_label)
 (block_label_start_expression)
] @label

[
 "#define"
 "#elif"
 "#endif"
 "#if"
 "#ifdef"
 (base_type_specifier)
 (contains_statement)
 (default)
 (derived_type_statement)
 (end_associate_statement)
 (end_block_construct_statement)
 (end_block_data_statement)
 (end_coarray_critical_statement)
 (end_coarray_team_statement)
 (end_do_loop_statement)
 (end_enum_statement)
 (end_enumeration_type_statement)
 (end_forall_statement)
 (end_function_statement)
 (end_if_statement)
 (end_interface_statement)
 (end_module_procedure_statement)
 (end_module_statement)
 (end_program_statement)
 (end_select_statement)
 (end_submodule_statement)
 (end_subroutine_statement)
 (end_type_statement)
 (end_where_statement)
 (enum_statement)
 (enumeration_type_statement)
 (enumerator_statement)
 (equivalence_statement)
 (function_statement)
 (implicit_statement)
 (interface_statement)
 (keyword_statement)
 (language_binding)
 (module_procedure_statement)
 (module_statement)
 (namelist_statement)
 (print_statement)
 (procedure_statement)
 (program_statement)
 (submodule_statement)
 (subroutine_statement)
] @keyword

(use_statement "use" @keyword)
(use_statement ["intrinsic" "non_intrinsic"] @keyword)
(included_items "only" @keyword)
(allocate_statement "allocate" @keyword)
(deallocate_statement "deallocate" @keyword)
(subroutine_call "call" @keyword)
(do_statement "do" @keyword)
(while_statement "while" @keyword)
(if_statement ["if" "then"] @keyword)
(elseif_clause ["else" "if" "elseif" "then"] @keyword)
(else_clause "else" @keyword)
(open_statement "open" @keyword)
(write_statement "write" @keyword)
(private_statement "private" @keyword)
(public_statement "public" @keyword)
(data_statement "data" @keyword)
(block_construct "block" @keyword)
(read_statement "read" @keyword)
(close_statement "close" @keyword)
(inquire_statement "inquire" @keyword)
(nullify_statement "nullify" @keyword)
(format_statement "format" @keyword)
(stop_statement ["error" "stop"] @keyword)
(where_statement "where" @keyword)

(select_case_statement "select" @keyword "case" @keyword)
(select_type_statement "select" @keyword "type" @keyword)
(select_rank_statement "select" @keyword "rank" @keyword)
(case_statement "case" @keyword)
(type_statement "type" @keyword)
(rank_statement "rank" @keyword)


[
 "*"
 "+"
 "-"
 "/"
 "="
 "<"
 ">"
 "<="
 ">="
 "**"
 "//"
 "=>"
 "=="
 "/="
 ".and."
 ".or."
 ".lt."
 ".gt."
 ".ge."
 ".le."
 ".eq."
 ".eqv."
 ".neqv."
 ".ne."
 ".not."
] @operator

;; Brackets
[
 "("
 ")"
 "["
 "]"
 "<<<"
 ">>>"
] @punctuation.bracket

;; Delimiter
[
 "::"
 ","
 "%"
 ":"
] @punctuation.delimiter

"&" @punctuation.special

(parameters
  (identifier) @variable.parameter)

(program_statement
  (name) @module)

(module_statement
  (name) @module)

(submodule_statement
  (module_name) (name) @module)

(function_statement
  (name) @function)

(subroutine_statement
  (name) @function)

(module_procedure_statement
  (name) @function)

(end_program_statement
  (name) @module)

(end_module_statement
  (name) @module)

(end_submodule_statement
  (name) @module)

(end_function_statement
  (name) @function)

(end_subroutine_statement
  (name) @function)

(end_module_procedure_statement
  (name) @function)

(end_type_statement
  (name) @type)

(subroutine_call
  (identifier) @function)

(interface_statement
  (name) @function)

(end_interface_statement
  (name) @function)

(method_name) @function

(binding_name
  (identifier) @function)

(keyword_argument
  name: (identifier) @keyword)

(keyword_statement
  (identifier) @label)

(derived_type_member_expression
  (type_member) @property)
