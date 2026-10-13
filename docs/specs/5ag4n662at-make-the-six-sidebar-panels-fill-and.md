---
task_uid: "01m4t5ezb2hkrshx5ag4n662at"
task: "T633"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test panel_width_tests::panels_follow_the_dock_width: with the sidebar dock at 360 px and at 180 px, each of the project, search, outline, git, references and problems panels draws its content within the dock's column bounds and as wide as the column (no 240 px fixed width, no overflow into the rail or past the window)"
    required: true
    manual: false
  - id: "C2"
    text: "the existing dock resize tests still pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Make the six sidebar panels fill and shrink with their dock's width instead of a fixed 240 px (T602 review F8)

## Problem

Follow-up from the codex review of T602 (F8, medium, follow-up): all six sidebar panels keep `.w(PANEL_WIDTH).flex_none()`, so their content stays 240 px wide whatever width the dock is resized to. Growing the dock leaves unused space beside the panel; shrinking it makes the panel content overflow into the activity rail or past the window edge. T602's resize tests assert the dock column's bounds but not the panel content's.

Sites (as of T602's candidate):
- `crates/fe-workbench/src/project_panel.rs` (~619)
- `crates/fe-workbench/src/search_panel.rs` (~1332)
- `crates/fe-workbench/src/outline_panel.rs` (~347)
- `crates/fe-workbench/src/git_panel.rs` (~456)
- `crates/fe-workbench/src/references_panel.rs` (~510)
- `crates/fe-workbench/src/problems_panel.rs` (~489)

## Change

- Each sidebar panel fills the width its dock allocates (`w_full()`/`flex_1` with `min_w_0` or the local equivalent) and may shrink below 240 px, clipping or truncating its rows rather than overflowing.
- `PANEL_WIDTH` stays only as the dock's default size, not a panel width.
- Tests go in the new `crates/fe-workbench/src/panel_width_tests.rs` (declared by a `mod` line in the crate), drawing the workbench with the sidebar dock at a width above and below 240 px and checking each panel's content bounds.
