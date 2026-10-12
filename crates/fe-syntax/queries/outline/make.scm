;; outline query, ours; capture convention of tree-sitter tags

; A rule's one target, as the whole rule, by its text: a file, an archive
; member, lib.a(a.o), or one with expansions, $(OUT)/stamp. Special
; targets, .PHONY and .SUFFIXES, start with a dot; pattern rules, %.o: %.c,
; have a %. Neither is listed.
((rule
  (targets . (_) @name .)) @definition.function
  (#not-match? @name "^\\.")
  (#not-match? @name "%"))

; Each of several targets, install uninstall:, on its own: one next to
; another target. The rule can't be the item of each, as one node is one
; item.
((targets (_) @name @definition.function . (_))
  (#not-match? @name "^\\.")
  (#not-match? @name "%"))

((targets (_) . (_) @name @definition.function)
  (#not-match? @name "^\\.")
  (#not-match? @name "%"))

(define_directive name: (word) @name) @definition.macro
