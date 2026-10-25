---
task_uid: "01m5qnx0v3mpap92cdfp3kg62v"
task: "T1144"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "trim_whitespace_tests::whole_document: editor.trim_trailing_whitespace removes trailing spaces and tabs from every row, including rows with only whitespace, as one undo step"
    required: true
    manual: false
  - id: "C2"
    text: "trim_whitespace_tests::caret_keeps_whitespace_before_it: a caret after 'x  |  ' keeps the two spaces before it and drops the two after; carets on several rows behave the same"
    required: true
    manual: false
  - id: "C3"
    text: "trim_whitespace_tests::no_change_no_edit: a clean document records no undo step and does not mark the buffer modified"
    required: true
    manual: false
  - id: "C4"
    text: "The editorconfig save path (existing editorconfig_save_tests and save_fixes_tests) uses the same model function and passes unedited"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap test trim_whitespace_key: 'secondary-k secondary-x' resolves to the command in the editor context; catalog tests updated; nextest and clippy pass"
    required: true
    manual: false
---

# Trim Trailing Whitespace command (cmd-k cmd-x), as VS Code's editor.action.trimTrailingWhitespace

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Trim Trailing Whitespace (editor.action.trimTrailingWhitespace, chord cmd-k cmd-x on macOS, ctrl-k ctrl-x elsewhere) is missing as a command. Trimming exists only as the editorconfig save step (`trim_trailing_whitespace`, `crates/fe-config/src/editorconfig.rs:42`, applied in `crates/fe-gpui/src/pane_item.rs:329-349`), so there is no way to trim on demand.

## Current state
- The save step in pane_item.rs trims the whole document when `.editorconfig` asks for it; look for the function it calls and reuse it rather than duplicating the scan.
- Chord bindings use `secondary-k <key>` (fastedit.toml lines 311-361, 741-761); `secondary-k secondary-x` is free.

## Change
- Register `editor.trim_trailing_whitespace` ("Trim Trailing Whitespace") in fe-editor (new `crates/fe-editor/src/trim_whitespace.rs`, mod line in `lib.rs`, register in `commands.rs`); the save step calls the same model function.
- VS Code semantics: trims spaces and tabs at the end of every row in the whole document; a row that holds an empty caret keeps the whitespace up to and including the caret column (only whitespace after the caret is removed), so typing a space and invoking it does not eat the space. One transaction, one undo step; no edit and no undo step when nothing changes. Selections and carets stay on their text.
- Keymap: `secondary-k secondary-x`, editor context (as the move_rows_down binding). No menu entry.

## Tests
New `crates/fe-editor/src/trim_whitespace_tests.rs`, `crates/fe-keymap/tests/trim_whitespace_key.rs`; catalog test lists updated.
