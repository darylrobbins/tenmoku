---
task_uid: "01m4r0ptmt07z1fmnsmz4a5c9v"
task: "T72"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test rows::tests::indent_and_outdent: indent on a selection over \"a\\n\\n  b\\nc\" ending at column 0 of row 3 indents rows 0 and 2 by tab_size spaces and skips the blank row 1 and row 3; outdent removes one tab or up to tab_size spaces; the selection still covers the same text; each is one undo step"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test rows::tests::tab_indents_multi_row_selections: tab with a selection over two rows indents both rows and keeps the selection; tab at a caret and on a single-row selection behaves as before"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test rows::tests::move_rows: move_rows_down on rows 0-1 of \"a\\nb\\nc\" gives \"c\\na\\nb\" with the selection on a and b; moving the last row of \"a\\nb\" up gives \"b\\na\"; at the top or bottom nothing changes; a folded range below is passed whole"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test rows::tests::duplicate_and_delete: duplicate_rows copies the touched rows below and moves the selection onto the copy; delete_rows on the middle of three rows leaves a caret at the same display column on the row that took its place, and on the last row also removes the break before it; each is one undo step"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-editor/src/tests/commands.rs lists the six new editor.* IDs as mutating, and cargo test -p fe-editor and cargo clippy -p fe-editor --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add row commands to fe-editor: indent and outdent rows, tab indenting a multi-row selection, move rows up and down, duplicate rows and delete rows (Mac T450)

## Problem

The plan's M6 is "Editing polish" (`~/.claude/plans/glittery-foraging-graham.md` line 110), and its reuse map lists
"Selection, cursor, movement, word, indent" (line 41). The editor has no row-level editing at all. The editor
registry (`crates/fe-editor/src/commands.rs` `registry()`, :236) has moves, selects, backspace, delete, newline,
`tab`, folds, clipboard, undo/redo and save, and nothing that indents or outdents a selection, moves rows, duplicates
them or deletes them. `EditorModel::tab` (`crates/fe-editor/src/lib.rs` :900) only inserts spaces at each caret, so a
selection over ten rows is replaced by spaces instead of being indented. 2026-10-11 scoping pass: none of these
commands is on the board.

## Change

New module `crates/fe-editor/src/rows.rs` (`mod rows;` in lib.rs) with an `impl EditorModel` block and
`rows::register(&mut registry)` called from `commands::registry()`, using the `add` helper as T426's
`multi_cursor::register` does. Each command catches up (`sync_display`) first, makes all of its edits as **one**
transaction of the invoking actor, and returns `Effect::Changed` only if the text changed.

"The rows a selection touches" are the rows from its start to its end, except that a non-empty selection ending at
column 0 of a later row doesn't touch that row. Selections touching the same or adjacent rows form one block.
Selections stay on the same text: they are anchored across the edit, not collapsed to carets.

Commands (ID: model method), all mutating:

- `editor.indent`: `indent_rows()`. Inserts `tab_size` spaces at the start of each touched row that isn't blank
  (blank = empty or whitespace only). A caret or selection end at a row's start moves right with the indentation.
- `editor.outdent`: `outdent_rows()`. Removes one level from each touched row: a leading tab, else up to `tab_size`
  leading spaces (fewer if the row has fewer). Rows with no leading whitespace are left alone.
- `editor.tab` changes: when any selection touches more than one row, it indents (as `editor.indent`) instead of
  replacing the selections with spaces. Carets and single-row selections behave as today.
- `editor.move_rows_up` / `editor.move_rows_down`: `move_rows(up: bool)`. Each block swaps with the row above
  (below); a folded range there counts as one row, so the block moves past the whole fold. If the topmost block is
  at row 0 (the bottommost at the last row), nothing moves. Line breaks stay correct at the end of the text: moving
  the last row of `"a\nb"` up gives `"b\na"`, with no trailing break added or lost.
- `editor.duplicate_rows`: `duplicate_rows()`. Inserts a copy of each block's rows below it; selections move onto
  the copy.
- `editor.delete_rows`: `delete_rows()`. Deletes each block's rows with their line break (for the last row, the
  break before it). Each selection becomes a caret on the row that takes the block's place, at the same display
  column as its head, clamped to the row's end.

Not in this card: key bindings and menus (a later card binds them), detected indentation (the next card makes
indent, outdent and tab use a file's own tabs or space width), vim.

## Tests

New file `crates/fe-editor/src/rows_tests.rs`, declared from rows.rs
(`#[cfg(test)] #[path = "rows_tests.rs"] mod tests;`), using `crate::tests::{parse, render}`. Update the expected
command list and the mutating list in `crates/fe-editor/src/tests/commands.rs`.

## Implementation notes

- Moving, duplicating and deleting treat a folded range as one row on the block's side too: a block touching a
  folded range takes in all of it, so moving a fold's header moves its hidden rows with it, and duplicating or
  deleting it copies or deletes the whole range. Indent and outdent touch only the rows selections touch.
- Moving swaps the block with its neighbour by moving the neighbour's text to the block's other side, so the
  block's own text (and anything anchored in it, such as another split's carets) stays put. Folds within the
  block and the passed range are folded again at their new rows when the edit drops them.
- A selection whose end was the start of the row after the block keeps covering the break only while there is
  one: moving `"a\n[b\n]c"` down gives `"a\nc\n[b]"`.
- Each command reports `Effect::Changed` and `run_as` drops it when nothing shown changed, as for the other
  commands.
- For the card that binds these: fe-gpui's vim treats any command starting `editor.move` as a motion
  (`MOTION_PREFIXES`), which takes in `editor.move_rows_up`/`_down`.
