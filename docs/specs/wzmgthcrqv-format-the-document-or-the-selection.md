---
task_uid: "01m4qzfbc83fmfckwzmgthcrqv"
task: "T475"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test lsp_format::tests::document_and_selection: lsp.format_document applies the fake server's edits as one step that cmd-z undoes; lsp.format_selection sends the selection widened to whole rows; both send the editor's tab size and insertSpaces false for a tab-indented file"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test lsp_format::tests::stale_missing_and_slow: typing before the reply drops its edits; no formatter shows No formatter for \u003clanguage\u003e; a timeout shows it; format_buffer reports Formatted, Unchanged or NoFormatter"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Format the document or the selection through its language server: lsp.format_document and lsp.format_selection, one undoable step

## Problem

The user (2026-10-12) asked for LSP formatting of a document or a selection. T470 adds `LspManager::format` and
T472 stubs `lsp.format_document` and `lsp.format_selection` (`Workbench::format_active` in
`crates/fe-workbench/src/lsp_format.rs`), which do nothing yet.

## Change

In `crates/fe-workbench/src/lsp_format.rs` only:

- `format_active(selection)`: for the active editor, `format` at the buffer's current version, with `None` for the
  document or the primary selection's range (widened to whole rows) for a selection; a caret with
  `lsp.format_selection` formats its row. Options from the editor: tab size and whether it indents with spaces (the
  editor's detected or configured indent style, T458, when there is one; spaces otherwise).
- The edits are applied to that buffer as one transaction of the user (as accepting a completion is: the editor's
  cmd-z undoes only its own origin, `EditorModel::undo`, `crates/fe-editor/src/lib.rs` :990, so `Origin::Format`
  would be out of the user's reach), if the buffer is still at that version; otherwise they're dropped (the user
  typed meanwhile) with nothing shown. cmd-z undoes a format as one step. Selections stay anchored.
- `Ok(None)`: an info prompt "No formatter for <language>". A timeout or error: an info prompt with it.
- `pub(crate) fn format_buffer(&mut self, editor, cx) -> Task<Result<FormatOutcome, String>>`, used by both
  commands and by the format-on-save card, with `FormatOutcome::{Formatted, Unchanged, NoFormatter}`.

## Tests

New file `crates/fe-workbench/src/lsp_format_tests.rs`, declared from lsp_format.rs with `#[path]`, on fe-lsp's fake
server.
