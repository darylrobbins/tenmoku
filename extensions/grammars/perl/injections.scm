; From tree-sitter-perl's queries/injections.scm at
; 4e56e286bbe60b1da410cf5cb3ebef75e134f30e, under its MIT licence (LICENSE).
; Changed: the heredoc injection, which takes the terminator as the language,
; is kept only for terminators naming a language the app has (ignoring case,
; as <<SQL or <<'HTML'); the injections into "comment", "pod" and, for s///e
; replacements, "perl" itself are dropped.

; inject a language into a heredoc body based on its terminator, so
; `my $q = <<SQL; ... SQL` highlights the body as SQL, `<<HTML` as HTML, etc.
((heredoc_content
  (heredoc_end) @injection.language) @injection.content
  (#match? @injection.language "^(?i:sql|html|css|javascript|js|json|xml|yaml|toml|bash|sh|python|ruby|lua|c|cpp|java|php|markdown|diff|ini|csv)$"))
