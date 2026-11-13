---
task_uid: "01m79cxff0dq8nc5faj6q6xqaz"
task: "T599"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "`lsp_semantic_tokens::inactive_tests::excluded_code_token_types_map_to_comment` and `lsp_semantic_tokens::inactive_tests::unknown_types_still_leave_the_grammar_colour` pass"
    required: true
    manual: false
---

# Dim inactive preprocessor code from a server's excluded-code semantic tokens (Mac T1771)

# Dim inactive preprocessor code from a server's semantic tokens (excluded code)

When a language server marks inactive `#if` branches through semantic tokens, show those rows dimmed. Server-agnostic: it is one
mapping in `highlight_name` (crates/fe-workbench/src/lsp_semantic_tokens.rs), which today returns `None` for an unknown token type and so
leaves the grammar's colour.

## What servers send (checked 2026-11-12)

- csharp-ls (card 1): supports semantic tokens, but its classification table (Handlers/SemanticTokens.fs) has no Roslyn
  `excluded code` entry, so unmapped classifications are dropped. It sends NOTHING for inactive code, so this card does not change
  how C# looks under csharp-ls. Do not promise otherwise in docs.
- Roslyn's own language server (Microsoft.CodeAnalysis.LanguageServer; MIT; NuGet package or the prerelease `roslyn-language-server`
  dotnet tool; not a standalone product) forwards Roslyn classification names as custom token types, which includes
  `excluded code` (ClassificationTypeNames.ExcludedCode; exact spelling from memory, NOT verified on the wire; the test must
  accept both `excludedCode` and `excluded code`). Registering it is a separate, optional card: awkward install, large, built to be
  driven by an editor extension.
- clangd (not a built-in server here) sends inactive lines as ordinary semantic tokens of type `comment`, no modifier, one token per
  line (SemanticHighlighting.cpp `toSemanticTokenType(InactiveCode) == "comment"`). `highlight_name("comment")` already maps that to
  `comment`, so it dims today with no change.

## Change

In `highlight_name`: `"excludedCode" | "excluded code" => "comment"`. No new theme token and no theme file edit: the name
`comment` is in the vocabulary and every theme colours it (a new name would take its longest listed prefix, `comment`, anyway, but
adding one to crates/fe-syntax/src/highlight.rs NAMES would ripple into fe-syntax tests and theme checks; avoid). Dimmed here means
comment-coloured, which is dim in Ember and the other themes. Update the doc comment above `highlight_name` to list the new types.

## Tests (new file only)

crates/fe-workbench/src/lsp_semantic_tokens_inactive_tests.rs, declared in lsp_semantic_tokens.rs next to `mod tests;` as
`#[cfg(test)] #[path = "lsp_semantic_tokens_inactive_tests.rs"] mod inactive_tests;` (pattern: lsp_semantic_tokens_tests.rs):

- `lsp_semantic_tokens::inactive_tests::excluded_code_token_types_map_to_comment`: both spellings give `Some("comment")`, with and
  without modifiers.
- `lsp_semantic_tokens::inactive_tests::unknown_types_still_leave_the_grammar_colour`: a made-up type still gives `None`.

(Confirm the full in-binary ids with the criteria-id rule; the module is declared inside `lsp_semantic_tokens`.)

## Notes for the lead

Linux-OK (label linux-ok): one match arm plus unit tests. Independent of card 1. Value is low until a server that sends it is wired
(Roslyn LSP) or clangd lands; file it as a small chore-sized feature, or park it. A grammar-side alternative for C# under
csharp-ls (dimming `#if false` branches from the tree-sitter parse) is a different, larger card and is not drafted. Overlaps
nothing in flight that I could find (scope is two files); T1737/T1739 touch fe-lsp/fe-workbench call sites, not this file, but check at lane-up.
