---
task_uid: "01m53q0rakagxy8wv7zw7ek4aj"
task: "T108"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "workbench.rs no longer has watch, sync_peers, follow_window_items, close_pane, PaneArea or the peers and pane_subscriptions fields; Workbench holds an Entity\u003ckiln_shell::Workspace\u003cPane\u003e\u003e made with the workbench key context, whose shell it renders and configures"
    required: true
    manual: false
  - id: "C2"
    text: "workspace_embed_tests.rs: a split, a tab moved by key to a pane's edge, a pane emptied by closing its last tab, and a toolbar request from a pane split off after launch each work through the Workspace (the last proving the PaneAdded subscription)"
    required: true
    manual: false
  - id: "C3"
    text: "No PaneGroup is held beside the Workspace: no field of Workbench or of fastedit's pane_group.rs wrapper has type kiln_shell::PaneGroup (or a clone of it), and Workbench::pane_group(cx) reads the workspace's"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench -p fastedit passes; the only edits to existing test files are pane_group() becoming pane_group(cx) in the scoped test files; pane(), shell(), dock(), terminal_dock() and status_bar() keep their signatures; no command ID, title or key context differs from the base"
    required: true
    manual: false
---

# Embed kiln's Workspace<Pane> in fastedit's Workbench and delete its copied pane glue and PaneArea (Mac T690)

Filed on argon from Mac T690 (card ~/coordination/argon-cards/zo/T690.json rev 4; note zo, the kiln phase-3 chain, card 2 of 17, serial). Mac task uid 01m4vbjhvpnj7gat7017c8xk10. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T107 (Mac T700), the card before. Mac deps already on the trunk: Mac T633. Held in the inbox: waits until Mac T509 (hot exit, workbench.rs) and Mac T542 (save reasons, pane.rs) land on Mac main and argon syncs past them; the Mac lead clears it. Title the host commit "... (Mac T690)".

## Problem

`Workbench` (`crates/fe-workbench/src/workbench.rs`, 2,996 lines on e776ee0e) carries a near-verbatim copy of
the pane glue that `kiln_shell::Workspace<P>` now owns (`kiln/crates/kiln-shell/src/workspace.rs`): `watch`
(~1392), `sync_peers` (~1438), `pane_changed` (~1449), the generic halves of `split`/`new_pane` (~1593-1619) and
`move_tab`/`move_tab_to` (~1620-1667), `follow_window_items` (~1706), `close_pane` (~1719), the `panes`, `peers`
and `pane_subscriptions` fields, and the `PaneArea` centre view (~2670-2696). The design record names this
copy as the composition tax the Workspace removes.

## Change

- `Workbench` holds an `Entity<kiln_shell::Workspace<Pane>>`, made with `Workspace::new(KEY_CONTEXT, first,
  new_pane, ..)` where it now makes its `Shell` (~447). The workspace's shell is the window's shell: the
  workbench sets its text size, chrome, menu bar, docks (the sidebar `Dock` and the terminal dock, placed with
  `shell.set_dock` as now), modal layer and fastedit's own `StatusBar` (`shell.set_status_bar`, replacing the
  workspace's built-in bar) on it. `render` draws the workspace where it drew `self.shell`.
- Delete `watch`, `sync_peers`, `pane_changed`'s close-on-empty, `follow_window_items`, `close_pane`, the generic
  part of `split`, `move_tab_to`, `PaneArea`, and the `peers`/`pane_subscriptions` fields. `split` calls
  `Workspace::split`; `move_tab` calls `Workspace::move_tab` (T700); `activate` calls `Workspace::activate` (T700)
  then does fastedit's part (quick open's and search's pane); `close_active_pane` keeps its confirm and calls
  `Workspace::close_pane`.
- The workbench subscribes to `WorkspaceEvent`: `PaneAdded` sets the new pane's toolbar state and subscribes to
  its `ToolbarRequest` (the fourth of today's per-pane subscriptions); `ActiveChanged` and `PaneRemoved` run
  `track_editors` and what `activate` did for the panes the workspace activates itself.
- One source of truth for the panes: the workspace's `PaneGroup<Pane>`. The workbench keeps no copy of it, and
  fastedit's wrapper in `pane_group.rs` no longer owns a group (it keeps only what fastedit adds: its rendering
  helpers and the test-only `moves` counter, now a `Workbench` field bumped by `move_tab`).
- `Workbench::pane_group()` becomes `pane_group(&self, cx: &App) -> &kiln_shell::PaneGroup<Pane>`, read from the
  workspace. Its callers change by that argument only: the 27 test files and 5 crate files that call it today
  (listed in the scope). The lead approved this one-time mechanical edit of existing tests (2026-10-16); no other
  test line changes. `pane()`, `shell()`, `dock()`, `terminal_dock()` and `status_bar()` keep their signatures:
  they return entity handles the workbench holds (`pane()` the active pane, which the workbench updates as it
  activates and on `ActiveChanged`).
- New tests in `crates/fe-workbench/src/workspace_embed_tests.rs`.

The window's saved layout stays on fastedit's own code here; T691 moves it.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited (T690's one-time `pane_group(cx)` change excepted); behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).

## Implementation notes

- `Workbench` holds `Entity<Workspace<Pane>>` (made with `KEY_CONTEXT`, the first pane and a factory that makes
  restored panes with `Pane::new_peer` from the active pane), the workspace's shell as `shell`, and the active
  pane as `active`, followed from the workspace. It observes the workspace (every pane change, close or
  activation notifies it) to follow the active pane (quick open's and search's pane) and run `track_editors`;
  `PaneAdded` sets the pane's toolbar state and subscribes to its `ToolbarRequest`; `PaneRemoved` drops that
  subscription.
- `restore_layout` keeps the window's own entry and docks, and has `Workspace::restore` put the panes back; the
  workbench ends its restore (activate, `adopt_orphans`) once the workspace stops restoring.
- `Workspace::new` gives its shell the platform's chrome; the workbench turns it off again (its host turns it on
  with `set_title_bar` and the like, as before). `Shell::text_size` is a builder, so the 12 px text size is set on
  a div around the workspace. The chrome's title stays the root's name (set as the workbench draws), not the
  active item's, which the workspace's `follow_active` sets.
- The workspace takes `PaneState::take_emptied` directly, so fastedit's guard (a pane whose last tab closes while
  a file is being read into it isn't emptied) moved from `Pane::take_emptied` to `Pane::items_changed`, which
  clears the flag then.
- `pane_group.rs`'s `PaneGroup` is now `pub type PaneGroup = kiln_shell::PaneGroup<Pane>`; `pane_group_mut` is
  gone. `moves` is a test-only `Workbench` field, bumped by `move_tab` and, through a test-only subscription,
  by each `TabDropped` (drops are the workspace's to move).
- Test-file edits beyond `pane_group()` → `pane_group(cx)`, each forced by it: the closures' unused `_` argument
  became `cx`; `pane_group_mut().ratios(..)` → `pane_group(cx).ratios(..)` (tests/pane_drag.rs,
  pane_drag_followup_tests.rs); `pane_group().moves` → `moves` (tests/pane_drag.rs); and
  `hot_exit_tests.rs`, which calls `pane_group()` since Mac T509 landed after the card was written.
  `launch_reading` takes `cx` too (search_replace.rs, workspace_edit.rs); vim_host.rs and external_drop.rs read
  the panes through `pane_group(cx)`.
