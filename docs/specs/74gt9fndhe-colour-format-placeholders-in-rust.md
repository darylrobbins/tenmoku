---
task_uid: "01m5qm99d3qeec3974gt9fndhe"
task: "T1123"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "New fe-workbench test file tests/lsp_semantic_format_specifier.rs: a fake server whose legend has formatSpecifier and escapeSequence answers tokens over the {} in a Rust string; the Foreground decorations are 'string.placeholder' for formatSpecifier and 'escape' for escapeSequence over the same ranges, and a variable token over the inlined name in {error} gives 'variable'. Test a_format_specifier_token_in_a_string_is_coloured_apart_from_the_string"
    required: true
    manual: false
  - id: "C2"
    text: "'string.placeholder' is in fe-syntax's highlight vocabulary and every bundled theme in crates/fe-config/themes colours it apart from its 'string' colour (the VS Code themes match VS Code's placeholder colour); new fe-config test tests/format_specifier_theme_key.rs asserts it for each bundled theme"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp TOKEN_TYPES lists formatSpecifier and escapeSequence as custom types and the initialize capability shows them advertised"
    required: true
    manual: false
  - id: "C4"
    text: "Evidence from real rust-analyzer on a log::warn!(\"a {} {error}\", x) line: the tokens it returns over the placeholders and over error, and that each now gets its decoration; if the inlined-name variable token was not shown before, the cause is named and fixed in scope"
    required: true
    manual: false
  - id: "C5"
    text: "Existing tests pass unedited (a vocabulary-count assertion may change); clippy passes"
    required: true
    manual: false
---

# Colour format placeholders in Rust strings as VS Code does: rust-analyzer's formatSpecifier and escapeSequence tokens and the inlined name in {error}, so log::warn!("... {} {error}") placeholders differ from the string

## Problem
The user reports (2026-10-24, screenshot of crates/fastedit/src/config.rs) that format placeholders in Rust log strings are not highlighted: in `log::warn!("extension {}'s grammars: {error}", manifest.id)` and `"extension {}'s grammar {} ({license})"` the whole literal is one string colour. In VS Code with rust-analyzer the placeholder braces get the `formatSpecifier` semantic token (scope constant.character.format.placeholder), escapes get `escapeSequence` (constant.character.escape), and an inlined name such as `error` in `{error}` gets its `variable` token, so all three differ from the string colour.

## Cause
crates/fe-workbench/src/lsp_semantic_tokens.rs `highlight_name` has no arm for rust-analyzer's custom token types and returns None (`_ => return None`), so no decoration is made and the tree-sitter string colour shows. tree-sitter-rust does not parse format strings, so only the server can supply this. The screenshot also shows `{error}` / `{license}` uncoloured although `variable` is a standard type: confirm with real rust-analyzer whether those tokens arrive and why they don't show, and fix it in scope.

## Change
- lsp_semantic_tokens.rs: map formatSpecifier to a new highlight "string.placeholder" and escapeSequence to "escape".
- fe-syntax highlight.rs: add "string.placeholder" to the vocabulary; every bundled theme in crates/fe-config/themes colours it (the dotted fallback would otherwise give the string colour); the VS Code themes use what VS Code shows for placeholders.
- fe-lsp semantic_tokens.rs: add both custom types to TOKEN_TYPES so the client capability advertises them.

## Tests
New files only: crates/fe-workbench/src/tests/lsp_semantic_format_specifier.rs (declared as `mod format_specifier` at the end of tests/lsp_semantic_tokens.rs, next to `mod underline_colour`) and crates/fe-config/tests/format_specifier_theme_key.rs.

## Implementation notes

C4 evidence, real rust-analyzer (1.98.1 toolchain) on `log::warn!("a {} {error}", x);`
with formatSpecifier/escapeSequence advertised, line 3 tokens (col, len, text, type):
`log` namespace, `::` operator, `warn` macro, `!` macro, `"a ` string, `{` (18) formatSpecifier,
`}` (19) formatSpecifier, ` ` string, `{` (21) formatSpecifier, `error` (22, len 5) variable,
`}` (27) formatSpecifier, `"` string, `x` variable. So the `error` variable token does arrive.

Cause it was not shown: `foreground_spans` (fe-gpui/src/element.rs) dropped a Foreground
decoration whose name the theme does not colour, and the bundled fastedit themes leave
`variable` to the foreground (UNCOLOURED in fe-config/tests/editor_palette.rs), so the string
colour showed through. Fixed in `foreground_spans`: a recognised name the theme leaves uncoloured
now paints the theme foreground (test element::foreground_spans_tests).
