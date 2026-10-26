---
task_uid: "01m5tygzfvyqpg43vj75mnjg9z"
task: "T352"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cursor_history_tests::click_then_undo: set_selections to a caret at row 1, then row 8, then row 20: editor.cursor_undo returns to row 8 then row 1; editor.cursor_redo goes forward again; the text and the text undo stack are untouched"
    required: true
    manual: false
  - id: "C2"
    text: "cursor_history_tests::typing_not_recorded: typing ten characters on a row adds no entries; select_all and add-cursor commands each add one"
    required: true
    manual: false
  - id: "C3"
    text: "cursor_history_tests::redo_cleared_and_clamped: a new selection change after cursor_undo clears redo; an entry beyond a text shortened by edits is clamped to the document end; the stack is capped at 50"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap cursor_undo_keys: cmd-u (macos) / ctrl-u resolve to editor.cursor_undo in the editor context with vim off, and vim insert mode's ctrl-u still resolves to vim.key; catalog tests updated"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Cursor Undo / Cursor Redo (cmd-u): step back through earlier selections, as VS Code's cursorUndo (Mac T1159)

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Cursor Undo (cursorUndo, cmd-u on macOS, ctrl-u elsewhere) and Cursor Redo (cursorRedo, no default key) are missing: after clicking away or a long jump there is no way to step back through earlier cursor and selection states without undoing text. Verified: no selection history in fe-editor (`EditorModel::set_selections` at `crates/fe-editor/src/lib.rs:800` just stores), and nav.back / nav.forward (the jump list) are a separate, coarser mechanism.

## Current state
- Selections are read with `selections()` (`lib.rs:781`) and written with `set_selections` (`lib.rs:800`); edits move selections through anchors.
- `ctrl-u` is bound for vim insert/replace on non-macOS only (fastedit.toml:1220, context `editor && (vim.mode == insert || vim.mode == replace)`); `cmd-u` is free on macOS.

## Change
- New `crates/fe-editor/src/cursor_history.rs` (+ `mod` in `lib.rs`): a per-editor bounded stack (50) of selection sets with a redo stack, recorded when the user's selection changes: a click, a keyboard move that changes the primary selection by more than the caret's own typing (typing and deleting do not push; a command that changes the selection without text, such as select all, add cursor, a jump to a definition, does), coalescing consecutive entries that land in the same row. Equal sets are not pushed twice.
- Commands `editor.cursor_undo` ("Cursor Undo") and `editor.cursor_redo` ("Cursor Redo") restore the previous / next selection set, clamped to the current text (the entries are offsets that follow edits, as anchors, or are clamped if a text edit invalidated them); they do not touch the text or the text undo stack. A new user selection change after an undo clears redo.
- Keymap: `cmd-u` on macos and `ctrl-u` elsewhere with context `editor && !vim.enabled` (so vim keeps its own `ctrl-u` and `u`); cursor_redo unbound.

## Tests
New `crates/fe-editor/src/cursor_history_tests.rs`, `crates/fe-keymap/tests/cursor_undo_keys.rs`; catalog tests updated.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y68).
