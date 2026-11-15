; Ours. The regex grammar is injected into Ruby regex literals, `/a+b/i` and
; `%r{a|b}`: the literal's `string_content`, never its interpolations (`#{x}`,
; which stay Ruby). A regex literal is source text, so the content is exact,
; escapes included. The built-in native Ruby language has no injection query
; and is unchanged. `injection.combine-kinds` joins the literal's escape sequences,
; which the grammar makes siblings of its string_content, into the one layer.
; Run on the native grammar by injection::tests.

((regex
  [(string_content) (escape_sequence)] @injection.content)
  (#set! injection.language "regex")
  (#set! injection.combined)
  (#set! injection.combine-kinds "string_content escape_sequence"))
