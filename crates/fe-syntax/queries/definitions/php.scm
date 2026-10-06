;; definitions query, adapted from tree-sitter-php 0.24.2 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Follows outline/php.scm in the symbols query.
; Changed: the property pattern is upstream's, with each element its own
; item and only in a class or trait; promoted properties, enum cases and
; top-level variables are ours.

; protected string $name; in a class or trait, named without its $. An
; anonymous class's properties aren't listed.
(class_declaration
  body: (declaration_list
    (property_declaration
      (property_element (variable_name (name) @name)) @definition.field)))

(trait_declaration
  body: (declaration_list
    (property_declaration
      (property_element (variable_name (name) @name)) @definition.field)))

; A property its constructor declares: __construct(private int $id). It
; nests under the constructor, inside whose parameters it's written.
(class_declaration
  body: (declaration_list
    (method_declaration
      parameters: (formal_parameters
        (property_promotion_parameter
          name: (variable_name (name) @name)) @definition.field))))

(trait_declaration
  body: (declaration_list
    (method_declaration
      parameters: (formal_parameters
        (property_promotion_parameter
          name: (variable_name (name) @name)) @definition.field))))

(enum_case name: (name) @name) @definition.variant

; $registry = []; at the top of the file or of a namespace's braces.
; Assignments in functions and methods are locals.
(program
  (expression_statement
    (assignment_expression left: (variable_name (name) @name)) @definition.variable))

(namespace_definition
  body: (compound_statement
    (expression_statement
      (assignment_expression left: (variable_name (name) @name)) @definition.variable)))

; list($a, $b) = $pair; and [$a, $b] = $pair; there: each name is its own
; item.
(program
  (expression_statement
    (assignment_expression
      left: (list_literal (variable_name (name) @name) @definition.variable))))

(namespace_definition
  body: (compound_statement
    (expression_statement
      (assignment_expression
        left: (list_literal (variable_name (name) @name) @definition.variable)))))
