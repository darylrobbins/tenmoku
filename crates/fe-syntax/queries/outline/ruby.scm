;; outline query, adapted from tree-sitter-ruby 0.23.1 queries/tags.scm
;; (MIT); capture convention of tree-sitter tags
; Changed: only @definition.* and @name are kept (no @doc, #strip!,
; #select-adjacent! or references); a def directly in a class, module or
; class << self body is a method and any other a function; private def
; and the like count as directly in the body; constants are added.

[
  (class body: (body_statement
    [
      (method name: (_) @name)
      (singleton_method name: (_) @name)
      (alias name: (_) @name)
    ] @definition.method))
  (module body: (body_statement
    [
      (method name: (_) @name)
      (singleton_method name: (_) @name)
      (alias name: (_) @name)
    ] @definition.method))
  (singleton_class body: (body_statement
    [
      (method name: (_) @name)
      (singleton_method name: (_) @name)
      (alias name: (_) @name)
    ] @definition.method))
]

; private def secret; end, and private_class_method def self.x; end.
[
  (class body: (body_statement
    (call
      !receiver
      arguments: (argument_list
        [
          (method name: (_) @name)
          (singleton_method name: (_) @name)
        ] @definition.method))))
  (module body: (body_statement
    (call
      !receiver
      arguments: (argument_list
        [
          (method name: (_) @name)
          (singleton_method name: (_) @name)
        ] @definition.method))))
  (singleton_class body: (body_statement
    (call
      !receiver
      arguments: (argument_list
        [
          (method name: (_) @name)
          (singleton_method name: (_) @name)
        ] @definition.method))))
]

(method name: (_) @name) @definition.function

(singleton_method name: (_) @name) @definition.function

(class
  name: [
    (constant) @name
    (scope_resolution name: (_) @name)
  ]) @definition.class

(module
  name: [
    (constant) @name
    (scope_resolution name: (_) @name)
  ]) @definition.module

; VERSION = "1.0" at the top or directly in a class or module body, also
; through a scope, as Config::VERSION = "1.0" and ::TOP = 1, named by the
; last constant, and each constant of a chain there: A and B in A = B = 1,
; B's item nesting in A's. See #assigned-in? in outline.rs.
((assignment
  left: [
    (constant) @name
    (scope_resolution name: (constant) @name)
  ]) @definition.constant
  (#assigned-in? @definition.constant program class module))

; A, B::C = 1, 2, also chained, as in A, B = C = [1, 2]: each constant is
; its own item.
((assignment
  left: (left_assignment_list
    [
      (constant) @name @definition.constant
      (scope_resolution name: (constant) @name) @definition.constant
    ])) @_assignment
  (#assigned-in? @_assignment program class module))
