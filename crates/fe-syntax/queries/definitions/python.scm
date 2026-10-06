;; definitions query, ours; capture convention of tree-sitter tags
; Follows outline/python.scm in the symbols query.

; An assignment directly in a class body, annotated or not: x = 1, x: int.
(class_definition
  body: (block
    (expression_statement
      (assignment left: (identifier) @name) @definition.field)
    (#unchained? @definition.field)))

; Each name a chained or unpacking one binds: x, y and z in x = y = z = 1,
; and a, b and c in a, [b, *c] = 1, (2, 3). Only the name is the item, so
; the names are siblings.
((assignment left: (identifier) @name @definition.field)
  (#chained? @name class_definition))

([
  (pattern_list (identifier) @name @definition.field)
  (tuple_pattern (identifier) @name @definition.field)
  (list_pattern (identifier) @name @definition.field)
  (list_splat_pattern (identifier) @name @definition.field)
 ]
 (#unpacked? @name class_definition))

; A module-level assignment to a plain name. An all-caps one is the
; outline's Constant, which comes first and wins on the same node.
(module
  (expression_statement
    (assignment left: (identifier) @name) @definition.variable)
  (#unchained? @definition.variable))

; Each plain name of a chained one: m and n in m = n = 1, a Constant when
; it's all caps. When the first is, as in MAX = MIN = 3, the outline's
; Constant MAX is the whole statement, and MIN nests in it. Unpacking,
; a, b = 1, 2, isn't listed at module level.
((expression_statement
  (assignment
    left: (identifier) @name @definition.variable
    right: (assignment)))
  (#chained? @name module)
  (#not-match? @name "^[A-Z][A-Z0-9_]*$"))

((assignment
  right: (assignment left: (identifier) @name @definition.constant))
  (#chained? @name module)
  (#match? @name "^[A-Z][A-Z0-9_]*$"))

((assignment
  right: (assignment left: (identifier) @name @definition.variable))
  (#chained? @name module))
