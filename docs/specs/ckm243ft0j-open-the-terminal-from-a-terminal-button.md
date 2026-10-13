---
task_uid: "01m4sj2426szag5jckm243ft0j"
task: "T601"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/dock_rail_actions.rs: a right dock with two panels and one RailAction draws three rail buttons, the action's bottom within 1 px of the rail's bottom; a panel that joins the rail later draws above the action, which keeps its place"
    required: true
    manual: false
  - id: "C2"
    text: "dock_rail_actions.rs: clicking the action dispatches its command once (seen by a test command handler) and changes neither the dock's active panel nor its visibility; set_action_highlighted(id, true) draws it highlighted; its tooltip reads the title and the command's key from the dock's shortcut contexts, as a panel button's does"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench terminal_rail_button_tests.rs: a new window's sidebar rail has a Terminal button (Icon::FileTerminal) whose tooltip names Terminal and terminal.toggle's key; clicking it shows the bottom dock with the terminal focused and the button highlighted; clicking it again with the terminal focused hides the dock and the highlight"
    required: true
    manual: false
  - id: "C4"
    text: "terminal_rail_button_tests.rs: showing and hiding the terminal by terminal.toggle, and its shell exiting, update the button's highlight in the next frame"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p kiln-shell -p kiln-ui -p fe-workbench and cargo clippy -p kiln-shell -p kiln-ui -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Open the terminal from a Terminal button at the bottom of the sidebar's activity rail, highlighted while the terminal shows

## Problem

The user (2026-10-13): "Maybe a terminal button on the sidebar to open the pane."

The terminal lives in the bottom dock (`crates/fe-workbench/src/workbench.rs`, `terminal_dock`), opened by
`terminal.toggle` (ctrl-backtick). Its only button is on the bottom dock's own rail, and that joins only
after the first show (`TerminalPanel::always_on_rail` is false), so a new window has no visible way to open
a terminal. The activity rail on the window's right edge (the sidebar dock, `crates/fe-workbench/src/dock.rs`)
lists only that dock's own panels: kiln-shell's `DockState` (`kiln/crates/kiln-shell/src/dock.rs`) draws one
`RailButton` per registered panel and has no button that does something else.

## Change

### kiln-shell: rail actions

`kiln/crates/kiln-shell/src/dock.rs`:

- `RailAction { id: &'static str, icon: Icon, title: SharedString, command: &'static str }` and
  `DockState::add_action(RailAction)`: a rail button that isn't a panel of this dock. Clicking it dispatches
  `command` through `kiln_platform::command_actions::dispatch_invocation` (`Source::Mouse`), as the command palette does; the
  dock's own state doesn't change.
- `DockState::set_action_highlighted(id, bool)`: drawn highlighted (the rail's accent bar) while true.
- Its tooltip is the title and the command's key, looked up exactly as a panel button's is
  (`DockState::shortcut`).
- Actions draw at the bottom of a side dock's rail, below the panel buttons, which keep their places as
  References and Problems join. `kiln/crates/kiln-ui/src/activity_rail.rs` gains
  `ActivityRail::footer(buttons)`: buttons pinned to the rail's bottom. A bottom dock draws its actions after
  its panel buttons in the title row.
- `DrawnDock::rail_button_bounds` and `rail_icons` include action buttons (by their id), so tests can click
  them.

### fastedit: the terminal button

`crates/fe-workbench/src/workbench.rs`: the sidebar dock gets a `RailAction` with id `"terminal"`,
`Icon::FileTerminal` (the bottom dock's terminal icon), title "Terminal" and command `terminal.toggle`. It is
highlighted while the bottom dock shows; every path that shows or hides the terminal dock (toggle, a shell's
exit, `dock.toggle`-style hides, a restored layout) keeps it in step, and notifies the sidebar dock so the
rail redraws.

New tests: `kiln/crates/kiln-shell/tests/dock_rail_actions.rs`, and
`crates/fe-workbench/src/terminal_rail_button_tests.rs` declared from `workbench.rs` with `#[path]`.

Fits Mac main as is (no stack 6 needed): `terminal.toggle` and the bottom dock are already there. When stack 6
merges, `workbench.rs` merges with T579's changes, which are in the terminal op handlers, not the dock setup.
