; Ours. The WebAssembly C# extension's injections.scm is this file, kept equal
; by wasm_parity's csharp_extension_is_as_built_in, so the extension gets these
; regex injections too: the built-in regex grammar is hidden but present, and
; the extension finds it by name. And XML is injected into /// doc comments
; (below).
;
; The regex grammar is injected into the pattern argument of the static
; System.Text.RegularExpressions calls Regex.IsMatch, Match, Matches, Replace and
; Split (the second argument, the input comes first) and into the first argument
; of `new Regex(pattern)` / `new Regex(pattern, options)`. Plain `Regex.` only,
; no aliases or qualified names.
; Limit: verbatim strings (@"..") and raw strings ("""..""") are injected whole
; (a verbatim string's @ and quotes are part of the layer); an escaped string
; only when its content has no backslash, as only then does the host text equal
; the regex text. With a backslash it stays a plain string.

; Regex.IsMatch(s, @"\d+")
(invocation_expression
  function: (member_access_expression
    expression: (identifier) @_cls
    name: (identifier) @_fn)
  arguments: (argument_list
    .
    (argument)
    .
    (argument (verbatim_string_literal) @injection.content))
  (#eq? @_cls "Regex")
  (#any-of? @_fn "IsMatch" "Match" "Matches" "Replace" "Split")
  (#set! injection.language "regex"))

(invocation_expression
  function: (member_access_expression
    expression: (identifier) @_cls
    name: (identifier) @_fn)
  arguments: (argument_list
    .
    (argument)
    .
    (argument (raw_string_literal (raw_string_content) @injection.content)))
  (#eq? @_cls "Regex")
  (#any-of? @_fn "IsMatch" "Match" "Matches" "Replace" "Split")
  (#set! injection.language "regex"))

; Regex.IsMatch(s, "a+b")
(invocation_expression
  function: (member_access_expression
    expression: (identifier) @_cls
    name: (identifier) @_fn)
  arguments: (argument_list
    .
    (argument)
    .
    (argument
      (string_literal . (string_literal_content) @injection.content .)))
  (#eq? @_cls "Regex")
  (#any-of? @_fn "IsMatch" "Match" "Matches" "Replace" "Split")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))

; new Regex(@"\d+")
(object_creation_expression
  type: (identifier) @_cls
  arguments: (argument_list
    .
    (argument (verbatim_string_literal) @injection.content))
  (#eq? @_cls "Regex")
  (#set! injection.language "regex"))

(object_creation_expression
  type: (identifier) @_cls
  arguments: (argument_list
    .
    (argument (raw_string_literal (raw_string_content) @injection.content)))
  (#eq? @_cls "Regex")
  (#set! injection.language "regex"))

(object_creation_expression
  type: (identifier) @_cls
  arguments: (argument_list
    .
    (argument
      (string_literal . (string_literal_content) @injection.content .)))
  (#eq? @_cls "Regex")
  (#not-match? @injection.content "\\\\")
  (#set! injection.language "regex"))

; XML doc comments: a run of /// lines is one xml layer, each line without its
; ///, which keeps the comment colour, as does the text between tags. Through
; `injection.doc-prefix`, our own property: the grammar makes each line one
; comment node, /// included, and the property cuts the prefix and combines
; the lines on consecutive rows, so <summary> and </summary> on lines of
; their own pair up. A blank line or any code between /// lines starts a new
; layer.
; Limits: /** */ doc comments are not injected, nor //// lines. A doc block
; is a run of elements with no one root, which tree-sitter-xml parses with
; error recovery; tags and attributes still colour. An unclosed or misnested
; tag can leave the rest of its block plain.
((comment) @injection.content
  (#match? @injection.content "^///")
  (#set! injection.language "xml")
  (#set! injection.doc-prefix "///"))
