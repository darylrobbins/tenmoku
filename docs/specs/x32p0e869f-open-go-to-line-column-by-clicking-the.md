---
task_uid: "01m4sjfh77pjdv31x32p0e869f"
task: "T74"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench test status_bar_go_to_line::click_opens_go_to_line: with a 3-line file open and the caret on line 2, clicking the status-item:cursor bounds opens the go-to-line picker (Workbench::go_to_line_open) whose status reads 'Line 2 of 3'; typing 3 and enter puts the caret on line 3 with the editor focused"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test status_bar_go_to_line::click_with_vim_on: with vim on in normal mode, the click opens the picker; escape dismisses it with the editor focused, the mode NORMAL and the caret unchanged; j then moves the caret down"
    required: true
    manual: false
  - id: "C3"
    text: "The new tests assert the editor's cursor item's command is go_to_line.toggle; pane_item_tests::status_items_are_the_editors passes unchanged; no file under crates/fe-keymap changes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-gpui -p fe-workbench passes; cargo clippy -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Open Go to Line/Column by clicking the status bar's line and column (Mac T597)

Filed on helium from Mac T597 (~/coordination/helium-cards/T597.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zk.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-13): "Let's make some improvements to the status bar. Colored vim modes. Click on the line/col to
goto line. Maybe some fancier toolbar icons. Let's make it a fancier status bar than just pure text."

The cursor item (`Ln 12, Col 5`, id `status::CURSOR`) runs nothing when clicked: in
`crates/fe-gpui/src/pane_item.rs`, `status_items` builds the right-side items from `status::RIGHT` zipped with
`texts`, and only the language item has a command (`language.select`). The go-to-line picker exists
(`go_to_line.toggle`, `crates/fe-workbench/src/picker.rs` registry, `Workbench::toggle_go_to_line`,
`crates/fe-workbench/src/go_to_line.rs`); opened with an empty query its status line reads `Line {caret} of {lines}`.

kiln-shell already runs a clicked item's `command` as a key would (`kiln/crates/kiln-shell/src/status.rs`,
`run_item_command`), and `kiln_ui::StatusEntry` gives an item with a command a hover background and a pointer
cursor, so no framework change is needed.

## Change

In `crates/fe-gpui/src/pane_item.rs`, `status_items`:

- The cursor item's command is `Invocation::of("go_to_line.toggle")`. The picker is not prefilled: its existing
  `Line N of M` status already shows the current line, and an empty query keeps typing a number immediate.
- Build the right-side items with `StatusItem::new(id, StatusSide::Right, priority, text)` and `.command(…)`
  instead of the `StatusItem { … }` struct literal, so fields a later card adds to kiln-shell's `StatusItem`
  (icon, tooltip) don't need this code changed. Same ids, priorities, texts and order.

No keymap, menu or picker change: `go_to_line.toggle`'s keys stay as they are (T462 binds them), and nothing here
takes a key.

## Tests

New file `crates/fe-workbench/src/tests/status_bar_go_to_line.rs`, declared in
`crates/fe-workbench/src/tests/mod.rs`, using the helpers pattern of `tests/status_bar.rs` (`item_bounds`,
`simulate_click` on the `status-item:cursor` debug selector, as `language_picker_sets_the_language` does).
`crates/fe-gpui/src/pane_item_tests.rs::status_items_are_the_editors` keeps passing unchanged.

## Implementation notes

- With vim on, the picker's first escape enters its own normal mode (T72); the second dismisses it. The C2 test presses escape twice.
- The carried T26 F1/F2 findings (save completion after the originating split closes; per-window prompt queue) are save-path redesigns, out of this xs card's scope, and are left for their own cards.
