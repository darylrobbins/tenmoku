; Ours: written for fastedit, after tree-sitter-md's block query, so its
; patterns win where they capture the same node.

; The crate's query clears a fence's code with `@none`; we keep it
; `markup.raw`, so a fence in a language we lack still stands out.
(code_fence_content) @text.literal

; A quote's text takes the quote's colour; its `>` markers, nested in it,
; keep theirs.
(block_quote) @markup.quote

[
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (task_list_marker_checked)
  (task_list_marker_unchecked)
] @markup.list

(link_reference_definition
  (link_label) @markup.link.label)
