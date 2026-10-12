---
task_uid: "01m4r7w2wm6t894x68zkka0ye0"
task: "T79"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test rename::tests::prepare: a Range reply, a range-with-placeholder reply, defaultBehavior and null each give the right RenameTarget or None in bytes of the version, with a UTF-16 emoji before the symbol; a server error comes back with its message; a server without prepareProvider gets the word at the offset with no request sent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test rename::tests::rename_edits: a rename reply touching the open file and a closed one decodes to FileEdits for both; a server without renameProvider gives Unavailable with no request"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-lsp and cargo clippy -p fe-lsp --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Ask language servers to rename a symbol: prepareRename for the target and rename as decoded workspace edits, in fe-lsp (Mac T468)

## Problem

The user (2026-10-12) asked for LSP rename symbol. T465 (the fe-lsp groundwork) declares the capability and decodes
workspace edits; nothing sends `textDocument/prepareRename` or `textDocument/rename`.

## Change

In `crates/fe-lsp/src/rename.rs` (the stub module T465 adds), an `impl LspManager` block, built like `hover`
(`crates/fe-lsp/src/hover.rs` :61, `document_request` with the buffer's version):

- `prepare_rename(buffer, version, offset) -> impl Future<Output = Result<Option<RenameTarget>, LspError>>`:
  `RenameTarget { range: Range<usize>, placeholder: String }` in bytes of that version. A `Range` reply uses the
  range's text as the placeholder, `{ range, placeholder }` keeps the server's placeholder, `defaultBehavior` takes
  the word at the offset, and a `null` reply is `Ok(None)` (nothing renameable there). A server error stays
  `Err(LspError::Server { message, .. })`, so a UI can show "cannot rename this". A server without
  `prepareProvider` gets the word at the offset as the target, without a request.
- `rename(buffer, version, offset, new_name) -> impl Future<Output = Result<Vec<FileEdits>, LspError>>`: the
  `WorkspaceEdit` decoded with `decode_workspace_edit`. A `RENAME_TIMEOUT` constant of 10 s here (renames walk the
  project; the `lsp.timeouts` settings table isn't touched).
- A server without `renameProvider`: `Err(LspError::Unavailable("rename"))` from both, with no request.

## Tests

New file `crates/fe-lsp/src/rename_tests.rs`, declared from rename.rs with `#[path]`, on the fake server.

## Implementation notes

- `LspManager::admit` and the manager's `clock` field became `pub(crate)` (manager.rs), so rename.rs can answer a server without `prepareProvider` from the admitted text with no request, and apply `RENAME_TIMEOUT` on the manager's clock.
- A `null` rename reply is `Ok(vec![])`. A `prepareRename` timeout is `Err(Timeout)`.
