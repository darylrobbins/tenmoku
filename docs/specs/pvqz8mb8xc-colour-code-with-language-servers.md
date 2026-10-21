---
task_uid: "01m55v39w0pknh3xpvqz8mb8xc"
task: "T175"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp_diagnostics::semantic_tokens::server_tokens_recolour_and_follow_edits passes: after opening, semanticTokens/full is sent and its parameter and function+defaultLibrary tokens become Foreground decorations variable.parameter and function.builtin; an edit then 250 ms sends full/delta with the resultId and the new tokens replace the old; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests tests::lsp_diagnostics::semantic_tokens::refresh_and_the_setting and lsp::semantic_tokens::tests::token_types_map_to_highlight_names pass (the mapping table row by row; a server-specific type maps to none)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Colour code with language servers' semantic tokens over tree-sitter's colours, after edits and on refresh, behind editor.semantic_highlighting, as VS Code does (Mac T946)

Filed on argon from Mac T946 (card ~/coordination/argon-cards/zzr/T946.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty06t2rt2vv0qtme7je4h. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T173 (Mac T944), T174 (Mac T945). Held in the inbox: waits until Mac T923, Mac T922 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T946)".

## Problem

LSP completeness track, item 6 (user-approved 2026-10-17). Once the cards this depends on land: an editor
recolours text by highlight name (`DecorationKind::Foreground`), fe-lsp asks for a buffer's semantic tokens
(full, then delta: `LspManager::semantic_tokens`) and reports `workspace/semanticTokens/refresh` as
`LspEvent::Refresh`, and `editor.semantic_highlighting` (default true) exists. Nothing connects them. VS Code
asks after an editor opens and after edits (debounced), re-asks on refresh, and maps token types and modifiers
to theme scopes; with the setting off it shows only the grammar's colours.

## Change

1. New `crates/fe-workbench/src/lsp_semantic_tokens.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_semantic_tokens.rs"] mod semantic_tokens;`, run from `Workbench::sync_lsp`:
   - **Mapping** (a table, `fn highlight_name(token_type, modifiers) -> Option<&'static str>`), to fe-syntax's
     highlight names: namespace → `module`; class, struct, enum, interface, type, typeParameter → `type`;
     parameter → `variable.parameter`; variable → `variable`; property, event → `property`; enumMember →
     `constant`; function → `function`; method → `function.method`; macro → `function.macro`; decorator →
     `attribute`; regexp → `string.special`; keyword, comment, string, number, operator → themselves. Then
     modifiers: `defaultLibrary` picks the `.builtin` variant (`type.builtin`, `function.builtin`);
     `documentation` on a comment → `comment.documentation`; `readonly` or `static` on a variable →
     `constant`. Any other type (a server's own) → none, so tree-sitter's colour stays.
   - **Per buffer** on a server with a `semanticTokensProvider`, with the setting on: ask when the buffer opens
     on its server, 250 ms after its last edit, and at once on `Refresh { what: SemanticTokens }` from its
     server, passing the previous answer for a delta. A landed answer for the buffer's current version
     replaces one `DecorationSet` provider per buffer, shared by its editors, holding a `Foreground`
     decoration per mapped token; an answer for an older version is dropped (the decorations already shown
     follow edits until the next one, as in VS Code).
   - The setting turning off removes the providers and stops asking; on, asks again. A buffer leaving its
     server removes its provider.
2. `crates/fe-workbench/src/lsp.rs`: the `sync_lsp` call, the `Refresh` arm, the setting read.
3. Tests: new `crates/fe-workbench/src/tests/lsp_semantic_tokens.rs`, declared at the end of
   `crates/fe-workbench/src/tests/lsp_diagnostics.rs` as `#[path = "lsp_semantic_tokens.rs"] mod
   semantic_tokens;`, with the fake of `tests/lsp.rs` advertising a legend:
   - `server_tokens_recolour_and_follow_edits`: after opening `src/main.rs` the fake gets
     `semanticTokens/full`; its `parameter` and `function`+`defaultLibrary` tokens become `Foreground`
     decorations named `variable.parameter` and `function.builtin` over their bytes; an edit then 250 ms send
     `full/delta` with the first `resultId`, and the new tokens replace the old.
   - `refresh_and_the_setting`: a `workspace/semanticTokens/refresh` sends a new request at once;
     `editor.semantic_highlighting = false` removes every `Foreground` decoration and edits send nothing.
   - New unit test file `crates/fe-workbench/src/lsp_semantic_tokens_tests.rs`, declared at the end of
     `lsp_semantic_tokens.rs`: `lsp::semantic_tokens::tests::token_types_map_to_highlight_names` checks the
     table above row by row, and that a server-specific type maps to none.

Linux-OK.
