---
task_uid: "01m516qsd2958h59ayxj98ndwg"
task: "T99"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "None of Shell::new, PaneState::new, ItemPane::new, DockState::new or Dock::new takes a UiTheme, and their docs no longer mention an unused argument"
    required: true
    manual: false
  - id: "C2"
    text: "Every caller is updated by dropping the argument only: the diff of each test file touches constructor calls and unused ui locals, no assertion"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p kiln-shell -p kiln -p fe-workbench passes and fastedit's screenshot goldens are unchanged"
    required: true
    manual: false
---

# Drop the ignored UiTheme argument from Shell::new, PaneState::new, ItemPane::new, DockState::new and Dock::new (Mac T676)

Filed on argon from Mac T676 (card ~/coordination/argon-cards/T676.json rev 3; note zj). Mac task uid 01m4vbj71sr8gk1dh8c22bvwmw. Base: argon trunk 6a499d8 = mac/main b39ea29 synced (Mac T675 landed). Mac deps already on the trunk: Mac T675, Mac T658. Mac T609 is listed as a dep but still open on the Mac; the Mac lead says no dependency is left (note zj). Title the host commit "... (Mac T676)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`). Four kiln-shell constructors take a `UiTheme` they ignore, each documented
"`_ui` is unused, left for callers until a later API cleanup":

- `Shell::new(context, _ui, window, cx)` (`kiln/crates/kiln-shell/src/shell.rs:81-83`);
- `PaneState::new(_ui, cx)` (`pane.rs:233`) and `ItemPane::new(ui, cx)` (`pane.rs:783`);
- `DockState::new(position, _ui)` (`dock.rs:365`) and `Dock::new(position, ui)` (`dock.rs:1052`).

They draw from the active theme (`ui_theme(cx)`), so the argument only misleads, and every caller has to fetch a
theme to pass.

## Change

Remove the `UiTheme` parameter from those five constructors and the "unused" doc sentences, and update every
caller mechanically (drop the argument; drop a `ui` local that becomes unused):

- kiln: `kiln-shell/src/workspace.rs`, and the kiln-shell tests that call them (`tests/close.rs`,
  `command_palette.rs`, `dock_position.rs`, `dock_rail_actions.rs`, `dock_resize.rs`, `dock.rs`, `layout.rs`,
  `pane_group_resize.rs`, `pane_group.rs`, `pane.rs`, `shell.rs`, `theme_follow.rs`, `workspace_*.rs`,
  `item_state.rs`, and T658's `dock_full_height.rs` and `dock_no_rail.rs`), and the host example if it still
  calls one;
- fastedit: `crates/fe-workbench/src/workbench.rs` (`DockState::new` ~315, `Dock::new` ~342, `Shell::new` ~409),
  `pane.rs` (`PaneState::new` ~135) and `dock.rs` (`DockState::new` ~201).

No behaviour changes and no test assertion changes: a test's diff is only the dropped argument. `rg -n
"UiTheme" kiln/crates/kiln-shell/src/{shell,pane,dock}.rs` shows no constructor parameter afterwards.
