; From tree-sitter-liquid at e45dbac8c5fa95b1f0e00e7e0c04bc8855823391,
; https://github.com/hankthetank27/tree-sitter-liquid, path
; queries/injections.scm, license MIT. Unchanged apart from this header.

((template_content) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))

((json_content) @injection.content
  (#set! injection.language "json")
  (#set! injection.combined))

((style_content) @injection.content
  (#set! injection.language "css")
  (#set! injection.combined))

((stylesheet_content) @injection.content
  (#set! injection.language "css")
  (#set! injection.combined))

((js_content) @injection.content
  (#set! injection.language "javascript")
  (#set! injection.combined))

((front_matter) @injection.content
  (#set! injection.language "yaml"))
