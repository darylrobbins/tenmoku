---
task_uid: "01m4vbj2d4p90vdd1899fq6jt7"
task: "T670"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/workspace_chrome.rs: add_panel on the left, right and bottom shows each dock, and a rail tooltip shows the panel's show key bound in the workspace's context"
    required: true
    manual: false
  - id: "C2"
    text: "workspace_chrome.rs: toggle_dock hides and shows a dock; hiding one that holds focus, by toggle or by a rail click, moves focus to the active item"
    required: true
    manual: false
  - id: "C3"
    text: "workspace_chrome.rs: the status bar shows the workspace's items over the active item's and redraws when the active item changes or set_status_item is called, with no notify from the host"
    required: true
    manual: false
  - id: "C4"
    text: "workspace_chrome.rs: a function given to add_status_source is called when the bar draws and its item shows"
    required: true
    manual: false
  - id: "C5"
    text: "Existing kiln-shell tests pass unedited"
    required: true
    manual: false
---

# Give kiln's Workspace its docks and a built-in status bar, with focus leaving hidden panels and no notify relays

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`). The previous card added `kiln_shell::Workspace<P>` with its panes; an app
still builds its docks and status bar by hand, as `kiln/crates/kiln/examples/host/app.rs` does:

- `DockState::new(..).size(..).shortcut_contexts([KEY_CONTEXT])`, `Dock::with_state`, `add_panel`,
  `shell.set_dock(..)` (app.rs:79-108), a toggle that moves focus out of a hidden panel (264-276), and a
  `DockEvent` subscription that does the same after a rail click (124-127);
- a `StatusBar` view with a weak back-reference (557-566), host items kept in a `StatusItems`, and push-model
  relays `self.status_bar.update(cx, |_, cx| cx.notify())` (399, 412).

## Change

Extend `kiln/crates/kiln-shell/src/workspace.rs`:

- **Docks.** `add_panel(DockPosition, Entity<impl DockItem>, window, cx)` makes that position's `Dock` on first
  use (its `DockState` with `shortcut_contexts([workspace context])`, so rail tooltips find the show key) and
  sets it on the shell. `dock(DockPosition) -> Option<&Entity<Dock>>` is the escape hatch;
  `set_dock_size(DockPosition, Pixels)` overrides the size. `toggle_dock(DockPosition, window, cx)` shows or
  hides it, and focus never stays in a hidden panel: it goes to the active item, after a toggle and after a rail
  click (`DockEvent`).
- **Status bar.** A built-in status bar view: the workspace's own items over the active item's
  (`StatusItems::items_for`), drawn with `status::status_bar`. `set_status_item(StatusItem, cx)` and
  `remove_status_item(id, cx)` redraw it; it redraws on `ActiveChanged` without the host doing anything.
  `add_status_source(fn(&App) -> Option<StatusItem>)` registers a function called each time the bar draws (a
  pulled item, for the phase-2 `#[kiln::status]`); keep such functions O(1) and say so in the docs.

New tests go in `kiln/crates/kiln-shell/tests/workspace_chrome.rs`.
