; From tree-sitter-gomod-orchard at 97801af57aac43d45fb68dab0ac47f0d0ab18bd1,
; https://github.com/grammar-orchard/tree-sitter-gomod-orchard, path
; queries/highlights.scm, license MIT. Changes: adds `tool` to the directive
; keywords; versions are @number rather than @string; adds module paths and
; tools (@module), toolchain names (@number), replace and ignore file paths
; (@string.special), godebug keys (@property), values (@constant) and their
; `=` (@operator), escape sequences, brackets and commas.

[
  "require"
  "replace"
  "go"
  "toolchain"
  "exclude"
  "retract"
  "module"
  "ignore"
  "godebug"
  "tool"
] @keyword

"=>" @operator

(comment) @comment

[
  (version)
  (go_version)
  (toolchain_name)
] @number

[
  (module_path)
  (tool)
] @module

(file_path) @string.special

(godebug_spec
  key: (godebug_str) @property
  "=" @operator
  value: (godebug_str) @constant)

(escape_sequence) @escape

[
  "("
  ")"
  "["
  "]"
] @punctuation.bracket

"," @punctuation.delimiter
