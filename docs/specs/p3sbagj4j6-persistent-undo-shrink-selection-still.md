---
task_uid: "01m6vc6e56zyy29rp3sbagj4j6"
task: "T514"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "shrink_steps_back_through_an_undo_history_import (crates/fe-editor/tests/expand_selection.rs): after expanding a caret twice, an undo history import with selection_state/restore_selection_state around it leaves the expansion history live: shrink gives 4..6, then 5..5, then nothing, and the history was imported (T483 F11)"
    required: true
    manual: false
  - id: "C2"
    text: "A stale expansion history (the text or selections changed since it was set) is not brought back by restore_selection_state; expand_grows_each_selection_and_shrink_steps_back and an_edit_or_a_click_forgets_the_history still pass"
    required: true
    manual: false
  - id: "C3"
    text: "column_select::import_tests::a_box_goes_on_after_an_undo_history_import and cursor_history::import_tests::cursor_undo_reaches_selections_from_before_an_import still pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-editor -p fe-vim pass"
    required: true
    manual: false
---

# Persistent undo: Shrink Selection still steps back after a delayed undo import (follow-up to T483)

Follow-up to T483 (persistent undo, lent from Mac T1411), from its final review's finding F11 (medium, `crates/fe-editor/src/lib.rs:1006`). T483 carries it. When T483 is cut back to the Mac, this card goes with it.

# Persistent undo: Shrink Selection still steps back after a delayed undo import

## Problem
`Buffer::import_undo` (`crates/fe-buffer/src/buffer.rs:1256`) rebuilds a file's history with real edits that take the text away and put it back, so the text ends as it began but the buffer's version moves on. `import_undo_into` (`crates/fe-gpui/src/pane_item.rs:1337`) keeps each editor's `selection_state()` across it and puts it back with `restore_selection_state` (`crates/fe-editor/src/lib.rs:988`). That restores the selections, the primary, the goals, the box selection (rebased with `column_box.at_version(version)`, `lib.rs:1005`) and the cursor history, but not the Expand Selection history (`EditorModel::expand_history`, `lib.rs:197`).

`ExpandHistory` (`crates/fe-editor/src/smart_select.rs:14`) holds while its `version` is the snapshot's and its `current` is the selections (`live_expand_steps`, `smart_select.rs:95`). Run Expand Selection before a delayed import lands (the read is asynchronous, and a window waiting for its root's settings defers it): the selections come back the same, but the version doesn't match, so the history is dropped and Shrink Selection does nothing. This is the editor's `editor.shrink_selection` and Vim's `vim.shrink_selection` (`crates/fe-vim/src/smart_select.rs`, which drives the same model history).

The review asked: "Include valid expansion history in the captured selection state and rebase its version after successful import. Add an Expand Selection → delayed import → Shrink Selection regression asserting restoration of the original selection."

## Change
- `selection_state()` also captures the expansion history, only if it is live then (its version is the snapshot's and `current` is the selections, as `live_expand_steps` checks). A stale one is captured as none.
- `restore_selection_state` puts it back at the current version (an `ExpandHistory::at_version` in `smart_select.rs`, as `ColumnBox::at_version` does for the box), after the selections are set, so `current` still equals them. A state ignored for its length (the early return) restores nothing, as now.
- `ExpandHistory` and its steps are offsets; the text is as it was, so the steps need no other change.
- Any edit or click after the import still drops the history, as `an_edit_or_a_click_forgets_the_history` checks.

## Tests
Add to `crates/fe-editor/tests/expand_selection.rs` (an integration test, so the regression check runs it at the base):
- `shrink_steps_back_through_an_undo_history_import` (the regression test): a `Buffer::new("")` with an `Origin::User` insert of `abcdefghijkl` gives the record (`export_undo`). An editor over `Buffer::new("abcdefghijkl")` with a caret at 5 and `Nested(vec![4..6, 2..8, 0..12])` expands twice (`2..8`). Take `selection_state()`, `buffer_mut().import_undo(record).unwrap()`, `restore_selection_state(state)`. `shrink` gives `Effect::Changed` and `4..6`, again gives `5..5`, a third gives `Effect::Nothing`. Then `buffer_mut().undo(|origin| *origin == Origin::User)` leaves the empty text, which shows the history was imported.
- `expand_grows_each_selection_and_shrink_steps_back`, `an_edit_or_a_click_forgets_the_history`, and T483's `column_select::import_tests::a_box_goes_on_after_an_undo_history_import` and `cursor_history::import_tests::cursor_undo_reaches_selections_from_before_an_import` still pass.

The regression test uses only APIs on main: `EditorModel::selection_state`, `restore_selection_state`, `buffer_mut`, `Buffer::export_undo` / `import_undo` and the file's `expand` / `shrink` helpers.

## Not in scope
Vim marks, folds, visual selections and Vim's goal columns across the import (T510). Other T483 follow-ups (T507, T508, T509, T512). Expansion history in other split editors of the buffer is restored the same way, through `import_undo_into`'s existing per-editor states, with no change to `pane_item.rs`.
