---
task_uid: "01m4tqtp5cx7wngkawba0ark7v"
task: "T658"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/dock_full_height.rs: with left, right and visible bottom docks, the side docks run from the main area's top to the status bar, and the bottom dock lies between them under the center"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell: a dock built without a rail draws no rail buttons or actions at the bottom or a side; shown it draws its panel's title and body, hidden nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench terminal_no_rail_tests.rs: after terminal.toggle shows and hides the terminal, the terminal dock draws no rail button; the sidebar's Terminal action still toggles it"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p kiln-shell -p fe-workbench and cargo clippy -p kiln-shell -p fe-workbench --all-targets -D warnings pass; the workbench screenshot golden matches (regenerated only for this change's diff)"
    required: true
    manual: false
---

# Run the side docks the full window height with the terminal only under the editor, and drop the terminal dock's own rail button

## Problem

The user (2026-10-13), looking at the app with the terminal open:

- "We'll still have a terminal toggle button floating on the left side." T601 put a Terminal button at the
  bottom of the sidebar's activity rail (a `RailAction` running `terminal.toggle`). The terminal dock still
  draws its own rail: `TerminalPanel::always_on_rail` is false, so after the first show its button joins
  the bottom dock's rail, which kiln-shell draws as a row of buttons under the panel at the window's bottom-left
  (`kiln/crates/kiln-shell/src/dock.rs`, `DockState::render`, the `DockPosition::Bottom` branch,
  `"dock-bottom-rail"`). It stays there when the terminal is hidden.
- "The side bar should extend all the way to the bottom of the screen including beside the terminal pane."
  `Shell::render` (`kiln/crates/kiln-shell/src/shell.rs`) lays out a row `Left docks | center | Right docks`,
  then the Bottom dock under that whole row, so the bottom dock spans the full window width and the side docks
  stop above it.

## Change

### kiln-shell: side docks run the full height

In `Shell::render`, the bottom dock goes under the center only: the main row becomes
`Left docks | column(center, Bottom docks) | Right docks`, each side dock running from below the title/top row
to the status bar. The bottom dock's width is the center's. Nothing else about docks changes: sizes, resize
edges (the dock-resize card's drag on the bottom dock's top edge still works), save/restore, and positions
set by `DockState::set_position` (T603).

### kiln-shell: a dock with no rail

`DockState::hide_rail(bool)` (builder `without_rail()` is fine too): a dock that draws no rail at any position:
no panel buttons and no actions; it draws only its open panel's column (title row and body), and nothing while
hidden. Its panels are shown by command or by another dock's `RailAction`. Default: a rail, as now.

### fastedit

The terminal dock (`crates/fe-workbench/src/workbench.rs`, `terminal_dock`) is built without a rail, in every
position T603 can move it to. The sidebar's Terminal button (T601) and `terminal.toggle` (ctrl-backtick)
are the ways to show it. Update T601's `terminal_rail_button_tests.rs` and `terminal_find_tests.rs` only where
they assert the terminal dock's own rail button.

### Goldens

Regenerate `crates/fe-workbench/examples/screenshots/golden/workbench.png` if it changes, after attributing the
diff to this change (see TESTING.md for the screenshot examples).

## Tests

- New `kiln/crates/kiln-shell/tests/dock_full_height.rs`: with a right dock, a left dock and a visible bottom
  dock, the side docks' bounds run from the top of the main area to the status bar's top, and the bottom
  dock's bounds lie between them horizontally (its left = left dock's right edge, its right = right dock's
  left edge), under the center.
- Same file or a new `tests/dock_no_rail.rs`: a bottom dock and a side dock built without a rail, with a panel
  that has shown, draw no rail buttons (no drawn rail items); shown, they draw the panel's title and body;
  hidden, nothing.
- New `crates/fe-workbench/src/terminal_no_rail_tests.rs` (declared from `workbench.rs` with `#[path]`):
  after `terminal.toggle` shows then hides the terminal, the terminal dock draws no rail button; the sidebar's
  Terminal action is still drawn and toggles it.

## Out of scope

Moving docks (T603), resizing (T602), the View menu item (T657).
