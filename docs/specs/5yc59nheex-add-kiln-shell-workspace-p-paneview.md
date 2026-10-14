---
task_uid: "01m4vbj1dp1ap8nf5yc59nheex"
task: "T669"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-shell tests/workspace_panes.rs: split right and down add a pane beside the active one showing a split copy of its item, active and focused; a pane whose last tab closes is removed unless it is the last; closing the active pane activates the nearest"
    required: true
    manual: false
  - id: "C2"
    text: "workspace_panes.rs: a tab dropped on another pane, or on a pane's edge, moves there (the split-off pane is watched like the others) and its destination becomes active"
    required: true
    manual: false
  - id: "C3"
    text: "workspace_panes.rs: the window title and the shell's title follow the active item's window_title, falling back to the workspace title, and ActiveChanged, PaneAdded and PaneRemoved are emitted as they happen; no fe-* crate or kiln example changes"
    required: true
    manual: false
  - id: "C4"
    text: "workspace_panes.rs: focus returns to the active item (or the empty active pane) through the workspace's and the centre view's Focusable, and closing the window asks the items of every pane"
    required: true
    manual: false
  - id: "C5"
    text: "workspace_panes.rs: Workspace works over a PaneView of the test's own, built through the new_pane factory and new_peer, as fastedit's Pane will need"
    required: true
    manual: false
---

# Add kiln_shell::Workspace<P: PaneView>: panes that split, close when emptied, take dropped tabs, follow the active item and give focus back

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`, from the docs card, which may not be on main yet).

Every kiln app with panes rewrites the same workspace glue. `kiln/crates/kiln/examples/host/app.rs` is 566
lines, about 400 of them generic: `ChatWindow`'s fields (47-65), `watch` (294-308), `sync_peers` (310-315),
`pane_changed` closing emptied panes (319-330), `tab_dropped` (333-352), `close_pane` (356-369),
`active_focus`/`focus_active` (373-382), `follow_active` (386-402), `split` (243-262), the `Panes` centre view
(526-553) and `close::set_window_items` (105-110). fastedit has a near-verbatim copy in
`crates/fe-workbench/src/workbench.rs`: `watch` (917), `sync_peers` (963), `pane_changed` (974), `split`
(1118), `follow_window_items` (1231), `close_pane` (1244) and `PaneArea` (~2134-2160), with
`pane_subscriptions: HashMap<EntityId, [Subscription; 4]>`.

## Change

Add `kiln_shell::Workspace<P: PaneView = ItemPane>` in a new `kiln/crates/kiln-shell/src/workspace.rs`
(`pub mod workspace;` and `pub use workspace::{Workspace, WorkspaceEvent};` in `lib.rs`). It is a view that
draws a `Shell` whose centre is its panes, and owns what the two copies above do by hand. This card covers the
panes; docks, the status bar and the saved layout follow in two more cards on the same file, so keep room for
them (no dock or status code here beyond what `Shell` already needs).

- **Construction.** `Workspace::new(context, first: Entity<P>, new_pane: impl Fn(&mut Window, &mut App) ->
  Entity<P> + 'static, window, cx)`, where `new_pane` makes the panes a restore needs (splits use
  `PaneView::new_peer`), and `Workspace::<ItemPane>::item_panes(context, window, cx)` for kiln's own pane.
  It builds the `Shell` under `context` with `Capabilities::current()` chrome; `.capabilities(..)` overrides it
  (tests use `Capabilities::for_os(Os::Linux)` as `tests/host.rs` does). `.title(..)` sets the fallback title.
- **Panes.** It holds the `PaneGroup<P>`, the active pane, the `Peers<P>` every pane sees, and one
  subscription set per pane (changes, `PaneEvent` activation, `TabDropped`), dropped with the pane. A pane its
  last tab left closes unless it's the last; a dropped tab moves through `PaneGroup::move_tab`, and a pane it
  splits off is watched. Methods: `split(Direction, window, cx)` (a split copy of the active item through
  `ItemHandle::split`, the new pane active and focused), `close_pane(&Entity<P>, window, cx)` (the nearest pane
  becomes active), `add_item(Box<dyn ItemHandle>, window, cx)` into the active pane,
  `activate_item_where(|&dyn ItemHandle| -> bool, window, cx) -> bool`, `active_pane()`, `active_item()`,
  `focus_active(window, cx)`.
- **Centre view and focus.** A generic centre view (the host's `Panes` and fastedit's `PaneArea`, once) renders
  the group in the workspace's context; `Focusable for Workspace` and for the centre view give the active item's
  focus handle, or the active pane's while it's empty, so a dismissed modal returns focus there.
- **Following the active item.** On every change of active pane or active item, the window title and the
  shell's title become the active item's `window_title`, else the workspace's title, and the workspace emits
  `WorkspaceEvent::ActiveChanged`. It also emits `PaneAdded(Entity<P>)` and `PaneRemoved(EntityId)`, so a host
  can add per-pane subscriptions of its own (fastedit's toolbar requests).
- **Closing.** `close::set_window_items` is wired to every pane's items, so closing the window and quitting ask
  them.
- **Escape hatches.** `shell()`, `group()`, `group_mut()`: the parts stay reachable as today.

fastedit and the host example are not changed in this card. New tests go in
`kiln/crates/kiln-shell/tests/workspace_panes.rs`.

## Implementation notes

- `activate_item_where` takes `Fn(&dyn ItemHandle, &App) -> bool`, not `Fn(&dyn ItemHandle)`: both copies'
  lookups read the item (the host's conversation's thread, fastedit's path), which needs `cx`.
- `.capabilities(capabilities, cx)` takes `cx`: the shell is already built, and an in-window menu bar is an
  entity. It sets the shell's title bar, menu bar and frame as `Shell::platform_chrome` would.
- `.title(..)` is a builder with no window, so `new` defers the first follow of the active item; it emits
  nothing then. `set_title(.., window, cx)` changes the fallback later.
- `split` returns the new pane. `new_pane(window, cx)` exposes the factory, for the restore card.
- `ActiveChanged` is emitted only when the active pane or its active item is another one; the titles are
  re-read on every pane change, so an item's retitle shows without an event. The first pane gets no
  `PaneAdded`: a host reads `group().panes()` at start.
- `close_pane` ignores a pane not in the group, so a stale close after a drop can't remove the wrong one.
- `close_pane` gives focus to the active item when the removed pane was active or held focus: a tab close
  that waited on its answer can empty a pane, focusing it, after another became active.
