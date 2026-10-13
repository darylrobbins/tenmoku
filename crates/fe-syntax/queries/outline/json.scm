;; outline query, ours; capture convention of tree-sitter tags

; Every pair, named by its key without the quotes: a module when its value
; is an object, else a field. The module pattern comes first, so it wins.
; Pairs in an object, or in objects in an array, nest under the pair whose
; value holds them.
(pair
  key: (string) @name
  value: (object)) @definition.module

(pair
  key: (string) @name) @definition.field
