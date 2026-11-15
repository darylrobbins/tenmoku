---
task_uid: "01m7c4m89r11wse5ftd0v1r7a1"
task: "T639"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench lsp::semantic_tokens::xml_doc_tests passes: each_xml_doc_type_maps_in_both_spellings, modifiers_do_not_change_xml_doc_mapping"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench lsp::semantic_tokens::inactive_tests passes unchanged; every name returned is in fe-syntax's NAMES; finish notes say whether csharp-ls sends these types"
    required: true
    manual: false
---

# Colour C# XML doc comment semantic tokens: map Roslyn's xmlDocComment* types (both spellings) to tag, attribute, string and comment.documentation in highlight_name (Mac T1836)

# Colour C# XML doc comment semantic tokens: map Roslyn's xmlDocComment* token types to tag, attribute and string

User-approved 2026-11-13. `crates/fe-workbench/src/lsp_semantic_tokens.rs`'s `highlight_name`
returns `None` for a server's own token types, which keeps the grammar's colour. A C# server built on
Roslyn's classifier sends doc-comment XML as its own types, so tags and attributes stay comment
colour even with the server running.

## What to build

Map Roslyn's XML doc comment classifications in `highlight_name`, in both spellings, as
`excludedCode` / `excluded code` already are: the LSP custom names (`xmlDocCommentName`, …) and the
raw classification names (`xml doc comment - name`, …):

| Roslyn type | Highlight |
|---|---|
| `xmlDocCommentName` | `tag` |
| `xmlDocCommentAttributeName` | `attribute` |
| `xmlDocCommentAttributeValue`, `xmlDocCommentAttributeQuotes` | `string` |
| `xmlDocCommentDelimiter` (also covers the `///`) | `comment.documentation` |
| `xmlDocCommentText` | `comment.documentation` |
| `xmlDocCommentComment` | `comment` |
| `xmlDocCommentCDataSection` | `markup.raw` |
| `xmlDocCommentEntityReference` | `escape` |
| `xmlDocCommentProcessingInstruction` | `keyword` |

- Check the names against Roslyn's source (`CustomLspSemanticTokenNames` and
  `ClassificationTypeNames`), and against the legend of the C# server Tenmoku runs by default,
  csharp-ls (`crates/fe-lsp/src/registry.rs`; `docs/decisions/2pj9dv7s78-csharp-server.md`). If
  csharp-ls folds doc comments into a standard type such as `comment`, say so in the finish notes:
  the mapping then helps Roslyn's own server (a manual `[lsp.servers]` entry) only. Every highlight
  name used must be one of fe-syntax's `NAMES` (`crates/fe-syntax/src/highlight.rs`).
- Update `highlight_name`'s doc comment.

## Tests

New `crates/fe-workbench/src/lsp_semantic_tokens_xml_doc_tests.rs`, declared from
`lsp_semantic_tokens.rs` as `#[cfg(test)] #[path = "lsp_semantic_tokens_xml_doc_tests.rs"] mod
xml_doc_tests;` (as `inactive_tests` is). Both spellings of each type; modifiers don't change the
answer; `lsp_semantic_tokens_inactive_tests.rs` passes unchanged.

## Notes

- Record design calls in the card's decision file, `docs/decisions/<lane>-*.md`.
- Linux-OK.
