; Ours: written for fastedit.
; Block Markdown on a Rust doc comment's line, for a quote or a list item
; (see rust-injections.scm). Their text is the comment's, under names of
; their own so a theme can tell them apart; the `>` and list markers too.
; The inline Markdown in them is `rustdoc_inline`, injected.

(block_quote) @comment.documentation.quote

[
  (block_quote_marker)
  (block_continuation)
] @comment.documentation

[
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (task_list_marker_checked)
  (task_list_marker_unchecked)
] @comment.documentation.list
