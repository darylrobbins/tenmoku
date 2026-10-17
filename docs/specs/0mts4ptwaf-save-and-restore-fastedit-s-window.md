---
task_uid: "01m53q0rk8hfwh1g0mts4ptwaf"
task: "T109"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "Workbench::set_layout, layout_entry, restore_layout, layout_id and restoring keep their signatures and delegate to the workspace's set_layout, entry, restore and restoring; close_layout, save_layout, the _layout_hooks field and render's save call are gone"
    required: true
    manual: false
  - id: "C2"
    text: "layout_compat_tests.rs: a saved entry in today's format (a JSON literal: two panes with tabs, the sidebar on the left, the terminal dock at the right with a set size, breadcrumbs off, bounds) restores those panes, tabs, positions, sizes and breadcrumbs, and saving it again gives an entry equal to the literal"
    required: true
    manual: false
  - id: "C3"
    text: "layout_compat_tests.rs: drawing three frames with nothing changed writes the layout store no times, and resizing the sidebar dock by its edge writes it once"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Save and restore fastedit's window layout through kiln's Workspace layout, restoring today's saved entries, and stop saving from render (Mac T691)

Filed on argon from Mac T691 (card ~/coordination/argon-cards/zo/T691.json rev 2; note zo, the kiln phase-3 chain, card 3 of 17, serial). Mac task uid 01m4vbjjzgzgdh1y74mq3secqj. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T108 (Mac T690), the card before. Title the host commit "... (Mac T691)".

## Problem

`Workbench` saves its own window layout (`set_layout`, `close_layout`, `layout_entry`, `restore_layout`,
`save_layout`: `crates/fe-workbench/src/workbench.rs` ~1836-1997), with its own quit and close hooks
(`_layout_hooks`), its own `restoring` flag, and a save attempted from `render` on every frame (~2612). Since
T690 its panes are a `kiln_shell::Workspace<Pane>`'s, which saves its layout itself on each shape change, at quit
and as the window closes (`Workspace::set_layout`/`entry`/`restore`/`on_save`/`on_restore`,
`kiln/crates/kiln-shell/src/workspace.rs` ~542-770).

## Change

- `Workbench::set_layout`, `layout_entry`, `restore_layout`, `layout_id` and `restoring` keep their signatures
  (the host in `crates/fastedit/src/main.rs` calls them) and delegate to the workspace's `set_layout`, `entry`,
  `restore` and `restoring`.
- fastedit's own keys go through `on_save`/`on_restore`: the sidebar and terminal positions, the sidebar dock's
  state (`dock`), the terminal dock's sizes (`terminal_dock`) and `breadcrumbs`; `bounds` as now. Today's entry
  names the two positions under `docks` (`{"sidebar": .., "terminal": ..}`), which is also the key the workspace
  writes its own docks under: fastedit's positions keep `docks` on restore of an old entry and are saved where
  the workspace's `docks` doesn't overwrite them, so an entry saved before this card restores the same.
- The sidebar dock's and terminal dock's changes (`_docks` subscriptions) ask the workspace to save, as its own
  docks do. Delete `close_layout`, `save_layout`, the `layout`, `bounds`-for-quit and `_layout_hooks` fields
  where the workspace covers them, and the save call in `render`.
- New tests in `crates/fe-workbench/src/layout_compat_tests.rs`.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited; behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).

## Implementation notes

- `Workbench::set_layout`, `layout_entry` and `restore_layout` call the workspace's `set_layout`, `entry` and
  `restore`. `layout_id` and `restoring` take no `cx`, so they can't read the workspace: the workbench keeps the
  id it handed over, and `restoring` mirrors `Workspace::restoring` (set as a restore starts, cleared in
  `follow_workspace` once the workspace's is done, where the workbench takes up the restored editors).
- fastedit's keys live in `LayoutKeys`, an `Rc` shared with the workspace's `on_save`/`on_restore`: the hooks
  can't read the `Workbench` entity, which is mid-update whenever it calls into the workspace. The keys hold the
  sidebar's dock, the terminal's dock and the shell (to move the docks on restore), the breadcrumbs and the
  bounds (kept from `observe_window_bounds`, no longer from `render`). The breadcrumbs are applied by
  `restore_layout` after the workspace's restore, as toggling them is the workbench's. An entry without panes
  (the workspace doesn't call `on_restore` for one) still puts back fastedit's keys, as before
  (`dock_position_tests::restoring_a_never_shown_terminals_sizes_leaves_it_off_the_rail`).
- kiln changes this needed (`kiln/crates/kiln-shell/src/workspace.rs`, outside the card's scope):
  - `Workspace::entry` merges its own docks into a `docks` object the app's keys give, instead of replacing it, so
    `{"sidebar", "terminal"}` stays where today's entries have it and an old entry saves again unchanged (C2).
  - `Workspace::save` (a full save now) for hot exit: a backup written or removed changes only the items'
    `state`, which `layout::shape` ignores, so it needs a full save as `write_layout(true)` did. The old
    `backups_changed` check (and `HotExit::saved`) is gone: a buffer turning dirty names its backup before it's
    written, but the entry saved then pointed at nothing on disk yet; the write itself saves in full, as does a
    removal, and a tab closing changes the items, which the workspace saves in full anyway.
  - `Workspace::save_at_quit`, which its own quit hook now calls, for `save_layout_at_quit` (kept, test-only,
    for `hot_exit_tests`' quit). Hot exit's own hooks (backups flushed at quit, ended as the window closes or the
    workbench goes) moved into `start_hot_exit`, registered before the workspace's so the backups are written
    before it saves at quit.
- The sidebar's and terminal's dock observers notify the workspace (`layout_keys_changed`), whose
  `observe_self` saves if the shape changed; toggling the breadcrumbs does the same.
- `joined` in a restored sidebar's state gains its active panel (`DockState::restore`, unchanged), so the C2
  literal is an entry saved after the project panel had shown (`"joined": ["project"]`).
- Carried T108 findings: F2, `PaneView::shown_item` (kiln, default the active item; fastedit's `Pane` gives
  `Pane::active_item`), used by `Workspace::split` and its focus of the active pane, so a split copies no tab
  hidden by a read or its error, and a close onto such a pane focuses the pane. F1, `follow_workspace` focuses
  the pane's `focus_target` when the workspace made another pane active itself (a click, a close, a file read
  in), as the workbench's own activation did; ordinary notifications move no focus. Tests are in
  `layout_compat_tests.rs`.
