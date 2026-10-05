;; outline query, ours; capture convention of tree-sitter tags

; Each heading, ATX or setext. The outline gives a heading the range of its
; section: down to the next heading of the same or a higher level in the
; same container (the document, a block quote or a list item), or to the
; container's end. The grammar's own sections don't serve: it makes them for
; ATX headings, but not reliably for setext ones.
;
; An empty ATX heading, such as #, has no content. Its name is empty, at the
; end of its marker.

(atx_heading (atx_h1_marker) heading_content: (_)? @name) @definition.heading.1

(atx_heading (atx_h2_marker) heading_content: (_)? @name) @definition.heading.2

(atx_heading (atx_h3_marker) heading_content: (_)? @name) @definition.heading.3

(atx_heading (atx_h4_marker) heading_content: (_)? @name) @definition.heading.4

(atx_heading (atx_h5_marker) heading_content: (_)? @name) @definition.heading.5

(atx_heading (atx_h6_marker) heading_content: (_)? @name) @definition.heading.6

(setext_heading heading_content: (_) @name (setext_h1_underline)) @definition.heading.1

(setext_heading heading_content: (_) @name (setext_h2_underline)) @definition.heading.2
