---
task_uid: "01m4ry9wtmrvf3kkdwftx9p8mv"
task: "T61"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test lsp_rename::tests::renames_across_files: on a symbol used in the open file and a closed one, lsp.rename shows the picker holding the placeholder, typing a new name and enter applies the fake server's edit to both files and returns focus with the caret on the renamed symbol; cmd-z in the editor undoes its part"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test lsp_rename::tests::nothing_and_failures: a null prepareRename shows Nothing to rename here; a server error shows its message; typing in the buffer before confirming refuses the rename with nothing changed; escape sends no rename"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Rename a symbol from the editor: lsp.rename asks the server for the target, takes the new name in a picker and applies the edit across files (Mac T473)

Filed on helium from Mac T473 (~/coordination/helium-cards/T473.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-z.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch Z to fill idle lanes; independent of helium's T47-T55 chain.

## Problem

The user (2026-10-12) asked for LSP rename symbol. T468 adds `prepare_rename` and `rename` to fe-lsp and T472 applies
workspace edits and stubs `lsp.rename` (`Workbench::rename_symbol` in `crates/fe-workbench/src/lsp_rename.rs`), which
does nothing yet.

## Change

In `crates/fe-workbench/src/lsp_rename.rs` only:

- `rename_symbol`: for the active editor's primary caret, `prepare_rename` at its buffer's version. `Ok(None)` or a
  server error: an info prompt (`window.prompt`) "Nothing to rename here" or the server's message. No server for
  the buffer: "No language server for this file".
- Otherwise a `RenamePicker` delegate shows as `Palette::Rename`: its input (through `Picker::input()`) holds the
  placeholder, all selected; one row, `Rename to <input>`, while the input is a non-empty name different from the
  placeholder; the status line says `Enter to rename, Escape to cancel`.
- Confirm: `rename(buffer, version, caret, new_name)` at the version the picker opened on, then
  `apply_file_edits`. Success: the picker closes and focus returns to the editor, whose caret stays on the renamed
  symbol. A stale version, a refused edit or a server error: an info prompt with the reason, and nothing changed.
  A second rename while one is running is refused.
- Escape cancels with nothing sent.

Not in this card: a key (F2), which the LSP key card binds.

## Tests

New file `crates/fe-workbench/src/lsp_rename_tests.rs`, declared from lsp_rename.rs with `#[path]`, on fe-lsp's
fake server as the workbench's other LSP tests use it.

## Implementation notes

- `lsp.rs` gained `Workbench::lsp_prepare_rename` / `lsp_rename` and a `Lsp::rename` guard: `Lsp`'s manager is private to that module, so lsp_rename.rs can't reach it otherwise.
- `TextInput` has no selection, so "all selected" is emulated: the first edit of the untouched placeholder replaces it.
- A buffer that changed since the picker opened is refused before any request is sent.
