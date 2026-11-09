; From tree-sitter-razor at d4664e409caaea12f73c9525484e3cf88b1cf718,
; https://github.com/tris203/tree-sitter-razor, path queries/injections.scm,
; license MIT. Changes: drops the injection of `@* *@` and `<!-- -->`
; comments into a `comment` language, which fe-syntax doesn't have. Each
; element's markup is HTML, combined with its sibling elements: an element
; inside a C# block has that block as its parent, so each block's markup is
; an HTML layer of its own, and the Razor and nested elements within an
; element are left out of its layer.

((element) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))
