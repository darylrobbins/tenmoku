---
task_uid: "01m4vbj33ygrvyerknbqka27eh"
task: "T671"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell tests/workspace_layout.rs: a workspace with split panes, registered items and docks, saved and restored into a new window, has the same pane tree, active pane, items and each dock's visibility, size and active panel"
    required: true
    manual: false
  - id: "C2"
    text: "workspace_layout.rs: after set_layout, a split, a pane close, a tab move and a dock toggle are each saved with no host call, and nothing is saved while a restore is still reading"
    required: true
    manual: false
  - id: "C3"
    text: "workspace_layout.rs: keys from on_save are in the saved entry and come back in the value restore reads"
    required: true
    manual: false
  - id: "C4"
    text: "workspace_layout.rs: the full entry is saved at quit and as the window closes, keeping the last window's layout; panes left empty once a restore fills close, and focus goes to the active item"
    required: true
    manual: false
  - id: "C5"
    text: "kiln-shell's layout.rs is unchanged and existing kiln-shell tests pass unedited"
    required: true
    manual: false
---

# Save and restore kiln's Workspace layout by itself: on each shape change, at quit and as its window closes

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`). `kiln_shell::Workspace<P>` now has panes, docks and a status bar, but an app
still wires its saved layout by hand, as `kiln/crates/kiln/examples/host/app.rs` does: `set_layout` with
`on_app_quit` and `on_window_closed` hooks (421-446), `entry()` (450-456), `save_layout` calls sprinkled into
commands (280, 400), `close_layout` (466-473), and `restore` with a `spawn_in` that waits for `restored.filled`
and then closes the panes left empty (475-506). fastedit does the same in `workbench.rs` (~1335-1497) and saves
from `render` (`workbench.rs:2079`).

## Change

Extend `kiln/crates/kiln-shell/src/workspace.rs` with the layout, composed from `kiln_shell::layout` as it is
(`save_panes`, `restore_panes`, `DockState::save`/`restore`, `WindowLayout`), without changing `layout.rs`:

- `set_layout(WindowLayout, window, cx)`: from then on the workspace saves its entry whenever its shape changes
  (`WindowLayout::save_if_changed`, after a split, a close, a tab move, an active change, a dock toggle, resize or
  panel switch), in full at quit (`on_app_quit`, `save_at_quit`) and as its window closes (`on_window_closed` and
  the workspace's release, whichever comes first, as app-framework.md §7 describes), keeping the last window's
  layout. Nothing is saved while a restore is still reading.
- `entry(cx) -> Value`: `{panes, docks: {left, right, bottom}}` plus the app's own keys from
  `on_save(|cx| -> Vec<(&'static str, Value)>)`.
- `restore(saved: &Value, window, cx) -> bool`: the panes (through `new_pane`), the active pane and each dock;
  false when the entry has no panes. The panes left empty once `restored.filled` resolves close, and focus goes
  to the active item. The app reads its own keys from the same `saved` value.

New tests go in `kiln/crates/kiln-shell/tests/workspace_layout.rs`.

## Implementation notes

- Shape changes are heard by observing the workspace itself (`observe_self`: splits, closes, tab moves, the
  active change and divider drags all notify it) and each dock (`observe`: toggles, resizes, panel switches),
  so no call site saves by hand. `set_layout`'s first save is deferred to the end of the update, so a restore
  in the same update, before or after it, isn't overwritten by the layout it replaces.
- While a restore reads, the full entry saved at quit or as the window closes is the entry being restored,
  not the empty panes. A second restore drops the first's pending fill.
- `restore` emits `PaneRemoved` for the panes it replaces and `PaneAdded` for the restored ones; an entry
  with no `panes` changes nothing.
- The shape alone misses a tab reordered in its pane (documents are numbered in traversal order), so the
  workspace also keeps each pane's item ids as last saved and saves in full when they change.
- A pane `restored.filled` reports empty closes only if it's still empty, and the close hooks count other
  windows by excluding the workspace's own window id, so a workspace released before its window keeps the
  last window's entry.
