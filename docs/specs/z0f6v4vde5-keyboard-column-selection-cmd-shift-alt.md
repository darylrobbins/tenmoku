---
task_uid: "01m5qp0mvdb4bgzzz0f6v4vde5"
task: "T1153"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "column_select_tests::down_grows_a_box: from a caret at row 1 column 3, editor.column_select_down twice gives three carets at column 3 on rows 1-3, and column_select_right then widens all three to columns 3-4 (selection set equals select_box((1,3),(3,4)))"
    required: true
    manual: false
  - id: "C2"
    text: "column_select_tests::short_rows: rows shorter than the box's left column get no selection and the box keeps its columns when moving over them; with every row short a caret sits at the head"
    required: true
    manual: false
  - id: "C3"
    text: "column_select_tests::box_ends_on_other_moves: any other selection change (a plain arrow, a click) clears the box so the next column_select starts from the new primary head; mouse alt-shift-drag tests still pass"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap column_select_keys: cmd-shift-alt-left/right/up/down resolve to the four commands on macos in the editor context and nothing is bound on other platforms; ctrl-shift-alt-down remains duplicate_rows"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog tests updated; cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Keyboard column selection: cmd-shift-alt-arrow extends a box selection, as VS Code's cursorColumnSelect*

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's keyboard column (box) selection, cursorColumnSelectLeft/Right/Up/Down (cmd-shift-alt-arrow on macOS), is missing. Mouse alt-shift-drag already selects a box; the keyboard cannot extend one. The Toggle Column Selection Mode command is not part of this card.

## Current state
- `EditorModel::select_box(anchor, head)` (`crates/fe-editor/src/multi_cursor.rs:423`) builds the selections for a box given an anchor and head as (row, display column); the mouse drag keeps the anchor in `Drag::Box` (`crates/fe-gpui/src/view.rs:322`, `:1357`).
- The model keeps no box state across commands. Existing neighbours: `editor.add_cursor_above/below` (fastedit.toml:172-177, cmd-alt-up/down on macOS) and `editor.split_selection_into_lines`.
- On non-macOS `ctrl-shift-alt-down` is already `editor.duplicate_rows` (fastedit.toml:245), so VS Code's Windows/Linux column-select keys collide; this card binds macOS only.

## Change
- fe-editor: new `crates/fe-editor/src/column_select.rs` (+ `mod` in `lib.rs`, register in `commands.rs`): a box state (anchor row, anchor display column, head row, head display column, which may exceed a row's length as VS Code's virtual space does for the box width) set by the commands and cleared by any other selection change. Commands `editor.column_select_left`, `_right`, `_up`, `_down` ("Column Select Left" etc.) start the box from the primary selection's head (anchor = head, column = its display column) if no box is active, move the head one column / row, and call `select_box`.
- Keymap: `cmd-shift-alt-left/right/up/down` on macos, editor context as the add_cursor bindings. Page up/down variants are out of scope.

## Tests
New `crates/fe-editor/src/column_select_tests.rs` (model-level), `crates/fe-keymap/tests/column_select_keys.rs`; catalog tests updated.
