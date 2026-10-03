; Goes after tree-sitter-ruby's highlights.scm for Ruby.
; From tree-sitter-ruby 0.23.1, https://github.com/tree-sitter/tree-sitter-ruby,
; path queries/highlights.scm, license MIT: its patterns that capture an
; `identifier`, restated in its order. Nothing else is added.
;
; The crate's query marks every identifier that isn't a local variable as a
; method, with `((identifier) @function.method (#is-not? local))`. We track no
; locals, so that matched every identifier, and local variables were
; highlighted as methods. Later patterns win over earlier ones on the same
; node: this marks identifiers as variables again, then repeats the crate's
; patterns that came after the method one.

(identifier) @variable

((identifier) @keyword
 (#match? @keyword "^(private|protected|public)$"))

(call
  method: [(identifier) (constant)] @function.method)

((identifier) @function.method.builtin
 (#eq? @function.method.builtin "require"))

(alias (identifier) @function.method)
(setter (identifier) @function.method)
(method name: [(identifier) (constant)] @function.method)
(singleton_method name: [(identifier) (constant)] @function.method)

((identifier) @constant.builtin
 (#match? @constant.builtin "^__(FILE|LINE|ENCODING)__$"))

(block_parameter (identifier) @variable.parameter)
(block_parameters (identifier) @variable.parameter)
(destructured_parameter (identifier) @variable.parameter)
(hash_splat_parameter (identifier) @variable.parameter)
(lambda_parameters (identifier) @variable.parameter)
(method_parameters (identifier) @variable.parameter)
(splat_parameter (identifier) @variable.parameter)

(keyword_parameter name: (identifier) @variable.parameter)
(optional_parameter name: (identifier) @variable.parameter)
