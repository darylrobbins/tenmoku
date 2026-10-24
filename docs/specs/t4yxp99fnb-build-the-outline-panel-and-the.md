---
task_uid: "01m5n9hz6axtbbjft4yxp99fnb"
task: "T211"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "With a server providing documentSymbol, the active editor's Outline rows and breadcrumb symbol chain are the server's symbols (same rows, icons, details, click-to-select-name as the tree-sitter outline); asked on open and 500 ms after edits settle, stale answers dropped"
    required: true
    manual: false
  - id: "C2"
    text: "No provider, an empty reply, an error or a stopped server falls back to the tree-sitter outline unchanged; existing outline panel and breadcrumb tests pass untouched"
    required: true
    manual: false
  - id: "C3"
    text: "A language with no outline query but a served outline shows the served one instead of Unsupported"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in lsp_symbols_tests.rs pass headless with the fake server"
    required: true
    manual: false
  - id: "C5"
    text: "The served symbols are keyed on the document session: after crash recovery under the same ServerId (new SessionId) and after the document is closed and reopened, the old symbols are dropped and re-asked without an edit, and a crashed server awaiting recovery is not served; two tests in lsp_symbols_tests.rs fail at the base and pass"
    required: true
    manual: false
---

# Build the Outline panel and the breadcrumb symbols from the language server's document symbols when it provides them, with tree-sitter as the fallback, as VS Code does (Mac T1108)

## Problem

Follows the fe-lsp document symbol card (`LspManager::document_symbols`). The Outline panel (`crates/fe-workbench/src/outline_panel.rs`) and the breadcrumb symbol chain (`pane_toolbar.rs`, `Workbench::push_symbols` in workbench_palettes.rs) are fed only by `fe_syntax::outline_job`, so an editor whose language has a server but no outline query shows a status ("Unsupported") and no symbols, and a file with a grammar shows tree-sitter's guess where the server knows better (rust-analyzer, gopls). VS Code's Outline and breadcrumbs use the document symbol provider and fall back to nothing; fastedit keeps tree-sitter as the fallback.

## Change

1. New `crates/fe-workbench/src/lsp_symbols.rs` (mod declared in lsp.rs beside the others, a `sync_symbols(cx)` call next to `sync_highlights`/`sync_lenses` in `sync_lsp`): for the active editor, once the buffer has rested 500 ms (VS Code's outline delay) after the last edit, and on open, ask `document_symbols(buffer, version)`; convert a `Some(non-empty)` answer into an `fe_syntax::Outline` (`name`, `detail`, `kind`, `range`, `name_range` = selection_range, `row` = the selection start's row, `depth`, `parent`, `truncated: false`, `version`) and hand it to the panel. A late answer for an old version is dropped; a request in flight is dropped (cancelled) when a newer one is made.
2. `outline_panel.rs`: `OutlinePanel::set_served(editor: EntityId, outline: Option<Outline>, cx)`. While an editor has a served outline the panel shows it (same rows, icons, details and click-to-jump as the tree-sitter one, `Status::Outline`) and breadcrumbs follow it through `shown_for`, whose generation moves with each served outline. With `None` (no provider, error, empty reply, server stopped, editor closed) the panel goes back to the tree-sitter outline exactly as today. A language with no outline query but a served outline shows the served one instead of `Unsupported`.
3. Nothing else changes: `pane_toolbar::symbol_chain` already takes an `&Outline`; the sticky-scroll model keeps using tree-sitter (not in scope).

## Tests

New `crates/fe-workbench/src/lsp_symbols_tests.rs` (declared at the end of lsp_symbols.rs with `#[cfg(test)] #[path]`, using `crate::tests::lsp::Lsp` and the fake server as lsp_highlights_tests.rs does): `server_symbols_replace_the_tree_sitter_outline` (panel rows are the server's names and nest), `breadcrumbs_follow_the_served_outline` (the pane toolbar's symbol chain at a caret), `clicking_a_served_row_selects_its_name_range`, `no_provider_keeps_the_tree_sitter_outline` (existing outline tests pass unchanged), `empty_reply_falls_back`, `language_without_an_outline_query_shows_served_symbols`, `edit_asks_again_after_the_rest_and_drops_stale_answers`, `switching_editors_shows_each_editors_own_outline`. Linux-OK (headless GPUI tests with the fake server; existing lsp tests run on Linux).
## Revision 2 (Mac lead decision on review F1, high): fix it

The symbols cache slot is keyed on the manager generation and `ServerId`, so stale document symbols survive automatic crash recovery (a new `SessionId` under the same `ServerId`, `server.rs:520`) and a document close/reopen. Key the slot on the document session instead: the manager's `active_identity(buffer, server)` (the `SessionId` plus the document's open epoch), validated against the identity captured when the request was made. A new session or a reopened document invalidates the slot and re-asks without needing an edit; while a crashed server waits for automatic recovery its symbols are not served (fallback to tree-sitter). Do not widen scope beyond the listed files; use only what `LspManager` already exposes. Tests (fail at the base): recover a crashed server under the same `ServerId` and check the old symbols are dropped and re-asked; close and reopen the document and check the same.
