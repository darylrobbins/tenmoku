; No injections. tree-sitter-fsharp's queries/injections.scm at
; aefd0c8741bdf3aeb827a228aa4a996a6536697e injects "markdown" into `(** *)`
; comments and "xml" into `///` comments, both by trimming the comment's
; opener with #offset!, which fe-syntax doesn't support, and "comment",
; which the app hasn't got; so none of its patterns is kept.
