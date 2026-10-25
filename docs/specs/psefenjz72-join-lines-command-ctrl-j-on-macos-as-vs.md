---
task_uid: "01m5qnvzyt89efzrpsefenjz72"
task: "T1140"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "join_lines_tests::bare_caret: 'a\\n   b\\nc' with the caret on row 0 gives 'a b\\nc', caret between 'a' and the space, one undo step"
    required: true
    manual: false
  - id: "C2"
    text: "join_lines_tests::selection_over_rows: a selection over three rows joins all three with single spaces; a selection ending at column 0 of the next row leaves that row alone"
    required: true
    manual: false
  - id: "C3"
    text: "join_lines_tests::no_space_cases: joining onto an empty row or a row starting with ')' inserts no space; the last row is a no-op with no undo step"
    required: true
    manual: false
  - id: "C4"
    text: "join_lines_tests::multiple_selections: two carets on separate rows each join in one transaction"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap test join_lines_key: ctrl-j resolves to editor.join_lines on macos in the editor context and is absent on other platforms; catalog tests updated; nextest and clippy pass"
    required: true
    manual: false
---

# Join Lines command (ctrl-j on macOS), as VS Code's editor.action.joinLines

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Join Lines (editor.action.joinLines, ctrl-j on macOS, no default on Windows/Linux) is missing in the editor outside vim. Verified: only fe-vim has `J` (`crates/fe-vim/src/edit.rs`); fe-editor has no join command. The macOS key ctrl-j is unbound in `crates/fe-keymap/keymaps/fastedit.toml`.

## Current state
- Row commands and the touched-rows `Block` are in `crates/fe-editor/src/rows.rs:25-120`.
- Bindings use the `context = "editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"` pattern (fastedit.toml, e.g. the move_rows_down binding near line 232).

## Change
- New `crates/fe-editor/src/join_lines.rs` (+ mod line in `lib.rs`, register in `commands.rs`): `editor.join_lines` ("Join Lines").
- VS Code semantics: an empty selection joins its row with the next; a selection over rows N..M joins them all (a selection that ends at column 0 of a later row does not pull that row in, and a selection within one row joins that row with the next). At each join the next row's leading whitespace is dropped and exactly one space is inserted, except when the current row is empty, the next row (after trimming) is empty, or it starts with `)`. The caret for an empty selection lands at the join point; a multi-row selection is kept over the joined text. Nothing happens on the last row. Multiple selections work as in rows.rs blocks, one transaction.
- keymap: `ctrl-j` on macos only, with the editor context above. No menu entry.

## Tests
New `crates/fe-editor/src/join_lines_tests.rs` and a binding test in a new `crates/fe-keymap/tests/join_lines_key.rs`; update the catalog tests.
