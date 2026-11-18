; Ours: written for fastedit, after tree-sitter-md's inline query, so its
; patterns win where they capture the same node. Its `text.strong` and
; `text.emphasis` are `markup.strong` and `markup.italic`; the `*`, `_` and
; `~` delimiters nested in them stay `punctuation.delimiter`.

; The crate's query colours a URL autolink but not an email one.
(email_autolink) @text.uri

(strikethrough) @markup.strikethrough

; A full reference's `[label]` is an identifier, not words. A collapsed
; one's `[text][]` text is both, and stays the link's: it's what's read.
(full_reference_link
  (link_label) @markup.link.label)
