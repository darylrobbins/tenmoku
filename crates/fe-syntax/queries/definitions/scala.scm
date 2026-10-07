;; definitions query, adapted from tree-sitter-scala 0.26.2 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Follows outline/scala.scm in the symbols query.
; Changed: a val, var or named given is a field in a class, trait, object or
; enum body and a variable at the top of the file or a package, rather than
; a variable anywhere, so a local val isn't listed; each name a val or var
; binds is its own item, the name itself, as a, b and c in
; val (a, (b, c)) = pair, at any depth;
; enum cases are variants; only a case class's first parameter list and
; val or var parameters are fields.

; At the top of the file or of a package. This comes before the field
; patterns, so it wins in a package's braces, a template body too.
[
  (compilation_unit
    [
      (val_definition pattern: (identifier) @name @definition.variable)
      (val_definition pattern: (identifiers (identifier) @name @definition.variable))
      (var_definition pattern: (identifier) @name @definition.variable)
      (var_definition pattern: (identifiers (identifier) @name @definition.variable))
    ])
  (package_clause
    body: (template_body
      [
        (val_definition pattern: (identifier) @name @definition.variable)
        (val_definition pattern: (identifiers (identifier) @name @definition.variable))
        (var_definition pattern: (identifier) @name @definition.variable)
        (var_definition pattern: (identifiers (identifier) @name @definition.variable))
      ]))
]

; The names a pattern binds there: a, b and c in val (a, (b, c)) = pair,
; and x in val Some(x: Int) = opt. See #bound? in outline.rs.
(
  [
    (tuple_pattern (identifier) @name @definition.variable)
    (typed_pattern pattern: (identifier) @name @definition.variable)
    (case_class_pattern pattern: (identifier) @name @definition.variable)
    (repeat_pattern pattern: (identifier) @name @definition.variable)
    (infix_pattern left: (identifier) @name @definition.variable)
    (infix_pattern right: (identifier) @name @definition.variable)
    (capture_pattern name: (identifier) @name @definition.variable)
  ]
  (#bound? @name compilation_unit package_clause))

; given ord: Ordering[Int] = ... there. An anonymous given has no name.
[
  (compilation_unit
    (given_definition name: (identifier) @name) @definition.variable)
  (package_clause
    body: (template_body
      (given_definition name: (identifier) @name) @definition.variable))
]

; In a class, trait, object or enum body, also abstract: val a, b: Int.
[
  (template_body
    [
      (val_definition pattern: (identifier) @name @definition.field)
      (val_definition pattern: (identifiers (identifier) @name @definition.field))
      (var_definition pattern: (identifier) @name @definition.field)
      (var_definition pattern: (identifiers (identifier) @name @definition.field))
      (val_declaration name: (identifier) @name @definition.field)
      (var_declaration name: (identifier) @name @definition.field)
    ])
  (enum_body
    [
      (val_definition pattern: (identifier) @name @definition.field)
      (val_definition pattern: (identifiers (identifier) @name @definition.field))
      (var_definition pattern: (identifier) @name @definition.field)
      (var_definition pattern: (identifiers (identifier) @name @definition.field))
      (val_declaration name: (identifier) @name @definition.field)
      (var_declaration name: (identifier) @name @definition.field)
    ])
]

(
  [
    (tuple_pattern (identifier) @name @definition.field)
    (typed_pattern pattern: (identifier) @name @definition.field)
    (case_class_pattern pattern: (identifier) @name @definition.field)
    (repeat_pattern pattern: (identifier) @name @definition.field)
    (infix_pattern left: (identifier) @name @definition.field)
    (infix_pattern right: (identifier) @name @definition.field)
    (capture_pattern name: (identifier) @name @definition.field)
  ]
  (#bound? @name template_body enum_body))

[
  (template_body
    (given_definition name: (identifier) @name) @definition.field)
  (enum_body
    (given_definition name: (identifier) @name) @definition.field)
]

; case class Point(x: Double): the parameters of a case class's first list
; are fields, after its name, type parameters, constructor annotations and
; constructor's access modifier, as x in case class C @Inject() (x: Int).
; Those of a later list, as ctx in case class C(x: Int)(ctx: S), aren't,
; unless val or var makes them fields below.
(class_definition
  "case"
  name: (identifier)
  .
  (annotation)*
  .
  class_parameters: (class_parameters
    (class_parameter name: (identifier) @name) @definition.field))

(class_definition
  "case"
  name: (identifier)
  .
  type_parameters: (type_parameters)
  .
  (annotation)*
  .
  class_parameters: (class_parameters
    (class_parameter name: (identifier) @name) @definition.field))

(class_definition
  "case"
  name: (identifier)
  .
  (annotation)*
  .
  (access_modifier)
  .
  class_parameters: (class_parameters
    (class_parameter name: (identifier) @name) @definition.field))

(class_definition
  "case"
  name: (identifier)
  .
  type_parameters: (type_parameters)
  .
  (annotation)*
  .
  (access_modifier)
  .
  class_parameters: (class_parameters
    (class_parameter name: (identifier) @name) @definition.field))

; class Circle(val radius: Double): plain parameters, as in class C(x: Int),
; aren't fields.
(class_parameter ["val" "var"] name: (identifier) @name) @definition.field

(simple_enum_case name: (identifier) @name) @definition.variant

(full_enum_case name: (identifier) @name) @definition.variant
