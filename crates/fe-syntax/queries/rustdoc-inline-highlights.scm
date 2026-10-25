; Ours: written for fastedit.
; Highlights for inline Markdown inside Rust doc comments. VS Code draws code
; spans, backticks included, brighter than the prose, and rust-analyzer's
; semantic tokens colour intra-doc links by what they resolve to. Until those
; arrive, link text is `markup.link`. Everything else is left to the comment.

[
  (link_text)
  (link_label)
] @text.reference

(shortcut_link
  (link_text) @text.reference)

; After the links, so the code inside [`Config`] takes this colour: later
; patterns win.
(code_span) @comment.documentation.code
