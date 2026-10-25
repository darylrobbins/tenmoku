---
task_uid: "01m5qnyvr6wcxkhpvh4w07e1sa"
task: "T1150"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "peek_references_tests::list_and_excerpt: editor.peek_references with a fake server returning 3 references in 2 files shows a list grouped by file (2 groups, 3 rows) and the first reference's source highlighted; Down selects the next and the excerpt follows"
    required: true
    manual: false
  - id: "C2"
    text: "peek_references_tests::enter_and_esc: Enter opens the selected reference at its position in the pane and closes the peek; Esc closes without moving the caret or opening a file"
    required: true
    manual: false
  - id: "C3"
    text: "peek_references_tests::find_all_still_fills_panel: nav.find_references still shows the dock References panel with the same rows as before (existing references_panel tests pass unedited) and opens no peek"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap peek_references_keys: shift-f12 resolves to editor.peek_references in the editor and shift-alt-f12 to nav.find_references in workbench; the terminal context keeps its own shift-f12; menu_go_nav and terminal_panel_tests assertions updated for the move"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog tests updated; nextest -p fe-workbench -p fe-keymap and clippy pass"
    required: true
    manual: false
---

# Peek References (shift-F12): references list with an inline excerpt in the peek widget; Find All References moves to shift-alt-F12, as in VS Code

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". In VS Code, shift-F12 is Go to References (editor.action.goToReferences), which opens the same inline peek widget with a list of references on the left and the selected one's source on the right; Find All References (editor.action.referenceSearch.trigger) is shift-alt-F12 and fills the side bar. Here shift-F12 runs `nav.find_references`, which fills the dock's References panel, and there is no inline peek.

## Current state
- Keymap: `shift-f12` -> `nav.find_references`, context `workbench && !terminal` (`crates/fe-keymap/keymaps/fastedit.toml:527`); menu entry `crates/fe-keymap/menus/fastedit.toml:415`; tests assert shift-f12 in `crates/fe-keymap/tests/menu_go_nav.rs:73` and `crates/fe-workbench/src/terminal_panel_tests.rs:242`.
- `Workbench::find_references` and `show_references` (`crates/fe-workbench/src/references_panel.rs:598`, `:627`) run the request through `nav_state` / the LSP and decode locations into `ReferencesPanel`'s file groups (`show_locations`).
- The peek shell (`PeekView`, reserved block, close behaviour) comes from the Peek Definition card.

## Change
- New command `editor.peek_references` ("Peek References"), key `shift-f12`, context `editor`: runs the same reference query as `find_references` but shows the results in the `PeekView`: a left list grouped by file (path, count; each row `line: text`) and the selected reference's source on the right with its range highlighted; Up/Down change the selection, Enter opens it in the pane, Esc closes. Results arrive incrementally the same way the panel's do.
- `nav.find_references` ("Find All References") keeps its id and behaviour but moves to `shift-alt-f12`, as in VS Code; the menu shows both. The vim `g r r` binding stays on `nav.find_references`. Update the assertions in `menu_go_nav.rs` and `terminal_panel_tests.rs` for the key move (the terminal-context binding of shift-f12 must still not steal it from the terminal).
- Reuse the panel's decode path (`show_locations`) rather than a second reference pipeline; no change to `ReferencesPanel` rows.

## Tests
New `crates/fe-workbench/src/peek_references_tests.rs`, `crates/fe-keymap/tests/peek_references_keys.rs`; update the two existing assertions named above.
