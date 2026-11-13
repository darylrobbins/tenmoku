; Adapted from nvim-treesitter's queries/verilog/highlights.scm at
; ca7993e6066077b1edcb71b82eb9bbaa61a7b90e (Apache-2.0, text in LICENSE-APACHE),
; the last revision written for tree-sitter/tree-sitter-verilog before
; nvim-treesitter moved to another grammar.
;
; Changes: dropped @spell, the earlier (include_compiler_directive)
; @constant.macro pattern a later one overrides, and the duplicate
; (type_declaration) pattern; begin/end and generate block labels are @label
; rather than @comment. Compiler directives capture their keyword (`define)
; rather than the whole directive (whose node takes in the whitespace and
; line break after it, and an include's path); system task and function
; names are @function.builtin wherever they're called, as statements too;
; real numbers (fixed-point and exponent forms, also in time literals) are
; @number.

; Keywords
[
  "begin"
  "end"
  (module_keyword)
  "endmodule"
  "program"
  "endprogram"
  "package"
  "endpackage"
  "checker"
  "endchecker"
  "pure"
  "virtual"
  "extends"
  "implements"
  "super"
  (class_item_qualifier)
  "parameter"
  "localparam"
  "defparam"
  "assign"
  "modport"
  "fork"
  "join"
  "join_none"
  "join_any"
  "default"
  "break"
  "assert"
  "tagged"
  "extern"
  (unique_priority)
] @keyword

[
  "class"
  "endclass"
  "interface"
  "endinterface"
  "enum"
  "struct"
  "union"
  "typedef"
] @keyword.type

[
  "function"
  "endfunction"
  "task"
  "endtask"
] @keyword.function

"return" @keyword.return

[
  (always_keyword)
  "generate"
  "for"
  "foreach"
  "repeat"
  "forever"
  "initial"
  "while"
] @keyword.repeat

[
  "if"
  "else"
  (case_keyword)
  "endcase"
] @keyword.conditional

(comment) @comment

(package_import_declaration
  "import" @keyword.import)

(package_import_declaration
  (package_import_item
    (package_identifier
      (simple_identifier) @constant)))

(text_macro_identifier
  (simple_identifier) @constant.macro)

(package_scope
  (package_identifier
    (simple_identifier) @constant))

(package_declaration
  (package_identifier
    (simple_identifier) @constant))

[
  "="
  "-"
  "+"
  "/"
  "*"
  "^"
  "&"
  "|"
  "&&"
  "||"
  "<="
  "@"
  "@*"
  "=="
  "!="
  "==="
  "!=="
  "-:"
  "<"
  ">"
  ">="
  "%"
  ">>"
  "<<"
  "|="
  (unary_operator)
  (inc_or_dec_operator)
  "#"
] @operator

(parameter_port_list
  "#" @constructor)

[
  "or"
  "and"
] @keyword.operator

(cast
  [
    "'"
    "("
    ")"
  ] @operator)

(edge_identifier) @attribute

(port_direction) @keyword.modifier

(port_identifier
  (simple_identifier) @variable)

[
  (net_type)
  (integer_vector_type)
  (integer_atom_type)
] @type.builtin

[
  "signed"
  "unsigned"
  "input"
  "output"
] @keyword.modifier

(data_type
  (simple_identifier) @type)

(method_call_body
  (method_identifier) @variable.member)

(interface_identifier
  (simple_identifier) @type)

(modport_identifier
  (modport_identifier
    (simple_identifier) @variable.member))

(net_port_type1
  (simple_identifier) @type)

[
  (double_quoted_string)
  (string_literal)
] @string

[
  "directive_begin_keywords"
  "directive_celldefine"
  "directive_default_nettype"
  "directive_define"
  "directive_else"
  "directive_elsif"
  "directive_end_keywords"
  "directive_endcelldefine"
  "directive_endif"
  "directive_ifdef"
  "directive_ifndef"
  "directive_line"
  "directive_nounconnected_drive"
  "directive_resetall"
  "directive_timescale"
  "directive_unconnected_drive"
  "directive_undef"
  "directive_undefineall"
] @keyword.directive

"directive_include" @keyword.import

; begin/end label
(seq_block
  (simple_identifier) @label)

[
  ";"
  "::"
  ":"
  ","
  "."
] @punctuation.delimiter

(conditional_expression
  [
    "?"
    ":"
  ] @keyword.conditional.ternary)

(default_nettype_compiler_directive
  (default_nettype_value) @string)

(text_macro_identifier
  (simple_identifier) @constant)

(module_declaration
  (module_header
    (simple_identifier) @constructor))

(class_constructor_declaration
  "new" @constructor)

(parameter_identifier
  (simple_identifier) @variable.parameter)

[
  (integral_number)
  (unsigned_number)
  (real_number)
  (fixed_point_number)
  (unbased_unsized_literal)
] @number

(time_unit) @type.builtin

(checker_instantiation
  (checker_identifier
    (simple_identifier) @constructor))

(module_instantiation
  (simple_identifier) @constructor)

(name_of_instance
  (instance_identifier
    (simple_identifier) @variable))

(interface_port_declaration
  (interface_identifier
    (simple_identifier) @type))

(net_declaration
  (simple_identifier) @type)

(lifetime) @keyword.modifier

(function_identifier
  (function_identifier
    (simple_identifier) @function))

(function_subroutine_call
  (subroutine_call
    (tf_call
      (simple_identifier) @function)))

(system_tf_identifier) @function.builtin

(task_identifier
  (task_identifier
    (simple_identifier) @function.method))

(type_declaration
  (data_type
    "packed" @keyword.modifier))

(enum_name_declaration
  (enum_identifier
    (simple_identifier) @constant))

(type_declaration
  (simple_identifier) @type)

[
  (integer_atom_type)
  (non_integer_type)
  "genvar"
] @type.builtin

(struct_union_member
  (list_of_variable_decl_assignments
    (variable_decl_assignment
      (simple_identifier) @variable.member)))

(member_identifier
  (simple_identifier) @variable.member)

(struct_union_member
  (data_type_or_void
    (data_type
      (simple_identifier) @type)))

(generate_block_identifier) @label

[
  "["
  "]"
  "("
  ")"
  "{"
  "}"
  "'{"
] @punctuation.bracket
