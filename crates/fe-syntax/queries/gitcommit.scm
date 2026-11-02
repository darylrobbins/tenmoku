; gitcommit highlights, ours; stands in for tree-sitter-git-commit's own query,
; whose captures are placeholders (`@subject`, `@plus`, ...). Later patterns win,
; so `comment` comes before the paths, branches and commits inside comments.

(comment) @comment

(subject) @markup.heading

(trailer
  key: (trailer_key) @property
  separator: _ @punctuation.delimiter
  value: (trailer_value) @string)

(path) @string.special

(branch) @label

(commit) @constant

(item) @markup.link

(change
  kind: _ @keyword)

(change
  [
    ":"
    "->"
  ] @punctuation.delimiter)

(scissors) @punctuation.special
