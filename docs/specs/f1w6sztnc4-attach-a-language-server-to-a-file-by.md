---
task_uid: "01m5phns1rq9vcs4f1w6sztnc4"
task: "T221"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A file whose language has no fe-syntax grammar attaches the language server configured for it: the workbench opens it, syncs it, and a fake server receives textDocument/didOpen and a request for it; a fake-server test in lsp_grammarless_tests.rs fails at the base and passes"
    required: true
    manual: false
  - id: "C2"
    text: "Languages that already attach keep their routing: the existing fe-lsp and fe-workbench LSP tests pass unchanged; cargo nextest run -p fe-lsp -p fe-workbench and cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings are clean; no Cargo change"
    required: true
    manual: false
---

# Attach a language server to a file by its language id, with or without an fe-syntax grammar (T212 review F2 follow-up)

## Problem

Follow-up to T212 (foldingRange), review F2 (high), split off by lead decision. A language server is attached by fe-syntax grammar name: `Document.language`, `LspManager::route_for` (`crates/fe-lsp/src/manager.rs:350`) and `Workbench::sync_lsp` (`crates/fe-workbench/src/lsp.rs:1332`, which derives the language from `view.syntax()`). A file whose language has no fe-syntax grammar therefore never reaches a language server, for any LSP feature (diagnostics, completion, folding, ...), even when a server is configured for it.

## Change

Attach a configured language server to a document by its language id (from the file's extension / the configured `[lsp]` entry), independent of whether fe-syntax has a grammar for it. Languages that already attach keep exactly their current routing. Add a fake-server integration test in the workbench: a file of a language with no grammar, with a server configured for it, opens, syncs, and receives a request (e.g. foldingRange from T212), and its folds come from the server.

## Notes

Cross-cutting: touches every LSP feature's entry point, so keep the change to the attachment decision and its tests. Split from T212 whose C3 documents this limit.

## Implementation notes

- The attachment decision is `routed_language` in `crates/fe-workbench/src/lsp.rs`: a buffer with a grammar routes by fe-syntax's name exactly as before; one without a grammar, whose path fe-syntax has no language for, routes by its file extension in lowercase, which a server's `languages` can list (`[lsp.servers.zls] command = "zls" languages = ["zig"]` takes `.zig` files). That string is the didOpen `languageId` too, as fe-lsp passes unknown names through.
- Not routed, as before: a buffer whose language is still loading, and a file fe-syntax has a language for that's shown as plain text (set by hand, or over the parse limit), so a grammar language keeps its routing.
- Nor is a buffer set to plain text by hand (review 36hsad F1): `EditorView::is_language_chosen` (`crates/fe-gpui/src/view.rs`, set by `set_language`, cleared by `detect_language` at open and after a Save As) turns the extension fallback off, so picking Plain Text closes the document on its server, also after picking a grammar language for a file of an unknown extension. Scope widened to `crates/fe-gpui/src/view.rs` and `pane_item.rs` for it.
- fe-lsp needed no change: `route_for`, `open` and `language_id` already take any string.
- Tests: `lsp_grammarless_tests.rs` (a `.zz` file with a `zzls` server configured is opened with `languageId` "zz", its edits synced, and its folds are the server's foldingRange; a `.yy` file with no server starts none; Plain Text by hand closes a `.zz` file and keeps it closed, also after Python by hand moved it to pyright).
