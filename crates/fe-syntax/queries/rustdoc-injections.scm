; Ours: written for fastedit. A doc comment's block Markdown (`rustdoc`)
; leaves its inline text to `rustdoc_inline`, as Markdown's block grammar
; leaves it to `markdown_inline`.

((inline) @injection.content
  (#set! injection.language "rustdoc_inline"))
