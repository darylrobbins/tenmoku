; Adapted from uben0/tree-sitter-typst's queries/typst/injections.scm at commit
; 46cf4ded12ee974a70bf8457263b67ad7ee0379d (MIT, see LICENSE). The shebang
; pattern and `#set-lang-from-info-string!` are dropped (not supported here);
; the info string is captured as `@injection.language` instead.

(raw_blck
  lang: (ident) @injection.language
  (blob) @injection.content)
