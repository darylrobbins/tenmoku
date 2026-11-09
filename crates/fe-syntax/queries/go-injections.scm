; Ours. The idea (inject the printf grammar into format strings) is from
; nvim-treesitter's queries/go/injections.scm, Apache-2.0; the patterns are
; written afresh. Injecting SQL into Go strings by their content is also an
; idea nvim-treesitter's Go injections use; those patterns and the regex below
; are written afresh too.
; The format is the first string argument of a Printf-style call, or the one
; after the writer, input or buffer argument for Fprintf, Fscanf, Sscanf
; and Appendf. The two
; patterns take disjoint name sets, so a value argument never injects.
(call_expression
  function: [
    (identifier) @_fn
    (selector_expression field: (field_identifier) @_fn)
  ]
  arguments: (argument_list
    . [
      (interpreted_string_literal (interpreted_string_literal_content) @injection.content)
      (raw_string_literal (raw_string_literal_content) @injection.content)
    ])
  (#match? @_fn "^[A-Za-z]+f$")
  (#not-any-of? @_fn "Fprintf" "Fscanf" "Sscanf" "Appendf")
  (#set! injection.language "printf"))

(call_expression
  function: [
    (identifier) @_fn
    (selector_expression field: (field_identifier) @_fn)
  ]
  arguments: (argument_list
    .
    (_)
    .
    [
      (interpreted_string_literal (interpreted_string_literal_content) @injection.content)
      (raw_string_literal (raw_string_literal_content) @injection.content)
    ])
  (#any-of? @_fn "Fprintf" "Fscanf" "Sscanf" "Appendf")
  (#set! injection.language "printf"))

; SQL by content, so database/sql, sqlx, pgx, sqlc output and custom wrappers
; all work. A string injects as a call argument, an expression_list value
; (const, var, :=, = and return) or a literal_element, never elsewhere.
; The call pattern skips functions ending in f: those are the printf patterns
; above, and a string gets one layer only (two over the same bytes would paint
; in a match order we don't control), so a format string holding SQL stays
; printf. An interpreted string injects only when its content is one piece: an
; escape sequence splits the content into pieces that would join wrongly
; without include-gaps, and with it the layer would get the backslash, so
; "SELECT a\nFROM t" stays plain. The case rule: both keywords of a statement
; are upper or both lower, which rejects sentence-case English such as
; "Select a file from the list". The text may open with whitespace and -- lines
; (sqlc's name comment). A bare SELECT needs upper case and a digit, *, ', (
; or a call after it. Backslashes are doubled: the query parser drops the
; backslash of any escape but \n \r \t \0.
(call_expression
  function: [
    (identifier) @_fn
    (selector_expression field: (field_identifier) @_fn)
  ]
  arguments: (argument_list
    [
      (raw_string_literal (raw_string_literal_content) @injection.content)
      (interpreted_string_literal . (interpreted_string_literal_content) @injection.content .)
    ])
  (#not-match? @_fn "^[A-Za-z]+f$")
  (#match? @injection.content "(?s)\\A(?:\\s*--[^\\n]*\\n)*\\s*(?:SELECT\\b.*\\bFROM\\b|INSERT\\s+INTO\\b|UPDATE\\s+\\S+\\s+SET\\b|DELETE\\s+FROM\\b|WITH\\s+(?:RECURSIVE\\s+)?\\w+\\s*(?:\\([^)]*\\))?\\s*AS\\s*\\(|(?:CREATE|ALTER|DROP)\\s+(?:UNIQUE\\s+)?(?:TABLE|INDEX|VIEW)\\b|SELECT\\s+(?:[0-9*'(]|\\w+\\()|select\\b.*\\bfrom\\b|insert\\s+into\\b|update\\s+\\S+\\s+set\\b|delete\\s+from\\b|with\\s+(?:recursive\\s+)?\\w+\\s*(?:\\([^)]*\\))?\\s*as\\s*\\(|(?:create|alter|drop)\\s+(?:unique\\s+)?(?:table|index|view)\\b)")
  (#set! injection.language "sql"))

([
  (expression_list
    [
      (raw_string_literal (raw_string_literal_content) @injection.content)
      (interpreted_string_literal . (interpreted_string_literal_content) @injection.content .)
    ])
  (literal_element
    [
      (raw_string_literal (raw_string_literal_content) @injection.content)
      (interpreted_string_literal . (interpreted_string_literal_content) @injection.content .)
    ])
]
  (#match? @injection.content "(?s)\\A(?:\\s*--[^\\n]*\\n)*\\s*(?:SELECT\\b.*\\bFROM\\b|INSERT\\s+INTO\\b|UPDATE\\s+\\S+\\s+SET\\b|DELETE\\s+FROM\\b|WITH\\s+(?:RECURSIVE\\s+)?\\w+\\s*(?:\\([^)]*\\))?\\s*AS\\s*\\(|(?:CREATE|ALTER|DROP)\\s+(?:UNIQUE\\s+)?(?:TABLE|INDEX|VIEW)\\b|SELECT\\s+(?:[0-9*'(]|\\w+\\()|select\\b.*\\bfrom\\b|insert\\s+into\\b|update\\s+\\S+\\s+set\\b|delete\\s+from\\b|with\\s+(?:recursive\\s+)?\\w+\\s*(?:\\([^)]*\\))?\\s*as\\s*\\(|(?:create|alter|drop)\\s+(?:unique\\s+)?(?:table|index|view)\\b)")
  (#set! injection.language "sql"))
