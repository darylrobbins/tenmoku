; WebAssembly text (.wat) and script (.wast) highlights, written for Tenmoku
; against wasm-lsp/tree-sitter-wasm's wast grammar at
; 2ca28a9f9d709847bf7a3de0942a84e912f59088 (upstream ships no queries).
; Later patterns win.

; Identifiers: a $name is a variable unless its place says otherwise.

(identifier) @variable

(module identifier: (identifier) @module)
(register (identifier) @module)
(action_invoke (identifier) @module)
(action_get (identifier) @module)
(script_module_binary (identifier) @module)
(script_module_quote (identifier) @module)
(meta_script (identifier) @module)
(meta_input (identifier) @module)
(meta_output (identifier) @module)

(module_field_func identifier: (identifier) @function)
(import_desc_func_type (identifier) @function)
(import_desc_type_use (identifier) @function)
(export_desc_func (index (identifier) @function))
(module_field_start (index (identifier) @function))
(elem_list (index (identifier) @function))
; Legacy element syntax, (elem $segment (i32.const 0) $f $g): the indices
; after the offset are functions; the segment's own name stays a variable.
(module_field_elem
  (offset)
  (index (identifier) @function))
(table_fields_elem (index (identifier) @function))
(instr_plain
  (op_index) @_op
  (index (identifier) @function)
  (#match? @_op "^(call|ref\\.func)$"))

(module_field_type identifier: (identifier) @type)
(type_use (index (identifier) @type))
(op_func_bind (index (identifier) @type))
(ref_type_ref (index (identifier) @type))
(instr_plain
  "ref.null"
  (index (identifier) @type))
(op_const_ref
  "ref.null"
  (index (identifier) @type))

(func_type_params_one (identifier) @variable.parameter)

(block_block (identifier) @label)
(block_loop (identifier) @label)
(block_if (identifier) @label)
(expr1_block (identifier) @label)
(expr1_loop (identifier) @label)
(expr1_if (identifier) @label)
(instr_plain
  (op_index) @_op
  (index (identifier) @label)
  (#match? @_op "^br"))
(instr_plain
  "br_table"
  (index (identifier) @label))

; Types.

[
  (num_type_i32)
  (num_type_i64)
  (num_type_f32)
  (num_type_f64)
  (num_type_v128)
  (ref_type_externref)
  (ref_type_funcref)
  (ref_kind)
  (elem_kind)
] @type.builtin

; Instructions.

[
  (op_nullary)
  (op_index)
  (op_index_opt)
  (op_index_opt_offset_opt_align_opt)
  (op_simd_offset_opt_align_opt)
  "br_table"
  "ref.null"
  "ref.extern"
  "call_indirect"
  "select"
  "let"
  "func.bind"
  "table.copy"
  "table.init"
  "v128.const"
] @function.builtin

(op_const
  (pat00) @function.builtin)
(op_const
  (pat01) @function.builtin)

; A NaN result, f32.const nan:canonical, and a lane instruction, as
; i8x16.extract_lane_s 0, are opcodes of several tokens: the whole node is the
; instruction, and its numbers (below) win.
[
  (result_const_nan)
  (op_simd_lane)
] @function.builtin

[
  (result_ref_func)
  (result_ref_extern)
  (result_ref_null)
] @function.builtin

; v128.const's lane shape.
(op_simd_const
  [
    "i8x16"
    "i16x8"
    "i32x4"
    "i64x2"
    "f32x4"
    "f64x2"
  ] @type.builtin)

; Control flow reads as keywords, as block, loop and if do.

((op_index) @keyword
  (#match? @keyword "^br"))
((op_nullary) @keyword
  (#match? @keyword "^(return|unreachable)$"))
"br_table" @keyword

; Keywords.

[
  "module"
  "type"
  "func"
  "param"
  "result"
  "local"
  "global"
  "memory"
  "table"
  "elem"
  "data"
  "start"
  "import"
  "export"
  "mut"
  "ref"
  "null"
  "offset"
  "item"
  "declare"
  "block"
  "loop"
  "if"
  "then"
  "else"
  "end"
  "binary"
  "quote"
  "register"
  "invoke"
  "get"
  "script"
  "input"
  "output"
  "assert_return"
  "assert_return_arithmetic_nan"
  "assert_return_canonical_nan"
  "assert_trap"
  "assert_exhaustion"
  "assert_malformed"
  "assert_invalid"
  "assert_unlinkable"
] @keyword

(share) @keyword

; Memory arguments: offset=4 align=4.

(offset_value "offset" @variable.parameter)
(align_value "align" @variable.parameter)
"=" @operator

; Literals.

(string) @string
(escape_sequence) @escape
(name (string) @string.special)

[
  (int)
  (float)
  (nat)
  (align_offset_value)
  (literal_nan_arithmetic)
  (literal_nan_canonical)
] @number

; Comments and annotations.

(annotation
  "(@" @attribute
  (identifier_pattern) @attribute)

[
  (comment_line)
  (comment_block)
  (comment_line_annot)
  (comment_block_annot)
] @comment

["(" ")"] @punctuation.bracket
