; Ours. Bash inside Makefile recipe lines.
; Recipe lines are bash. A line continued with `\` is several `shell_text`
; siblings under one `recipe_line`; combining them parses them as one script.
; Make references and `$$` escapes are named children of `shell_text`, so they
; stay out of the bash layer and keep their make colours.

((recipe_line (shell_text) @injection.content)
 (#set! injection.language "bash")
 (#set! injection.combined))
