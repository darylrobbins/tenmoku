---
task_uid: "01m4r7w2z83yq8wx1jgqhyn9yr"
task: "T80"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test code_action::tests::request_and_order: the request carries the range and the given diagnostics in the server's encoding with their data; a reply mixing Commands and CodeActions comes back with preferred quick fixes first, disabled ones kept with their reason"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test code_action::tests::resolve_edits_and_commands: an action without an edit is resolved through codeAction/resolve when the server supports it; its edit decodes with code_action_edits; run_code_action_command sends workspace/executeCommand with the command's arguments"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test code_action::tests::timeout_is_empty: no reply within 3 s on the manual clock gives an empty list and cancels the request"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-lsp and cargo clippy -p fe-lsp --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Ask language servers for code actions in fe-lsp: actions for a range and its diagnostics, quick fixes first, resolved, with their edits decoded and their commands run (Mac T469)

## Problem

The user (2026-10-12) asked for LSP code actions, quick fixes first. T465 declares the capability and decodes
workspace edits; nothing asks a server for code actions, resolves them or runs their commands.

## Change

In `crates/fe-lsp/src/code_action.rs` (the stub module T465 adds), an `impl LspManager` block:

- `code_actions(buffer, version, range: Range<usize>, diagnostics: Vec<Diagnostic>) -> impl Future<Output =
  Result<Vec<CodeAction>, LspError>>`. `range` is in bytes of `version`; `diagnostics` are the ones the host shows
  overlapping it (fe-lsp's own `Diagnostic`, re-encoded to the server's positions; their `data` kept as received).
  `CodeAction { title, kind: Option<String>, is_preferred: bool, disabled: Option<String>, diagnostics_fixed: usize,
  payload }`, where the opaque `payload` keeps what's needed to resolve and run it. A bare `Command` in the reply is a
  `CodeAction` with that title and no kind. Sorted: quick fixes for the given diagnostics first, preferred first,
  then the server's order. Timeout 3 s (as hover's), a timeout being an empty list.
- `resolve_code_action(&CodeAction) -> Future<Result<CodeAction, LspError>>`: `codeAction/resolve` when the server
  advertises `resolveProvider` and the action has no edit; otherwise it returns the action as is.
- `code_action_edits(&CodeAction) -> Result<Vec<FileEdits>, LspError>`: its edit decoded with
  `decode_workspace_edit` (empty if none).
- `run_code_action_command(&CodeAction) -> Future<Result<(), LspError>>`: sends `workspace/executeCommand` for its
  command, if any. Any edit the server applies while running it arrives as T465's `LspEvent::ApplyEdit`.
- Disabled actions are returned (with the reason) so a UI can grey them out; they are never run.

## Tests

New file `crates/fe-lsp/src/code_action_tests.rs`, declared from code_action.rs with `#[path]`, on the fake server.
