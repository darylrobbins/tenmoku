;; outline query, ours; capture convention of tree-sitter tags

; An ATX heading's section holds the heading and everything under it, down
; to the next heading of the same or a higher level, so sections nest by
; level. The grammar makes no section for a setext heading: its item is the
; heading alone, nested in the section around it.

(section (atx_heading (atx_h1_marker) heading_content: (_) @name)) @definition.heading.1

(section (atx_heading (atx_h2_marker) heading_content: (_) @name)) @definition.heading.2

(section (atx_heading (atx_h3_marker) heading_content: (_) @name)) @definition.heading.3

(section (atx_heading (atx_h4_marker) heading_content: (_) @name)) @definition.heading.4

(section (atx_heading (atx_h5_marker) heading_content: (_) @name)) @definition.heading.5

(section (atx_heading (atx_h6_marker) heading_content: (_) @name)) @definition.heading.6

(setext_heading heading_content: (_) @name (setext_h1_underline)) @definition.heading.1

(setext_heading heading_content: (_) @name (setext_h2_underline)) @definition.heading.2
