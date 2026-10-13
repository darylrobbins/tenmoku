;; outline query, ours; capture convention of tree-sitter tags

; Each stage's FROM instruction, by its alias, FROM rust:1 AS build, else by
; its image. Only the instruction: the ones after it aren't its range.
(from_instruction as: (image_alias) @name) @definition.module

(from_instruction (image_spec) @name !as) @definition.module
