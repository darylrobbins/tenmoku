---
task_uid: "01m4vbj3qgrbxapbchg1ezqcg3"
task: "T672"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/app_workspace.rs: an app with .item::\u003cT\u003e(), .panel(Left, ..) and .status(..) opens one Workspace window with the panel's dock and the status item; secondary-shift-p opens the palette and its picker keys work from kiln's defaults; with no menu table the default menus are installed"
    required: true
    manual: false
  - id: "C2"
    text: "app_workspace.rs: the layout is saved under the state dir's layouts, and a second start with the same paths restores the panes, items and dock; .restore_layout(false) opens a fresh window"
    required: true
    manual: false
  - id: "C3"
    text: "app_workspace.rs: in a debug build an unknown command in the app's keymap panics at start naming the file, and .config(|c| c.fail_fast(false)) turns that off"
    required: true
    manual: false
  - id: "C4"
    text: "app_workspace.rs: .root(..) still opens a Root window that saves no layout; with neither root nor any item, panel or workspace hook, start panics with a message naming .root and .item"
    required: true
    manual: false
  - id: "C5"
    text: "Existing kiln tests pass unedited"
    required: true
    manual: false
---

# Make a Workspace window the default of kiln::app, with item kinds, panels, status sources, the saved layout, and kiln's default keys, menus and debug fail-fast on

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`). Earlier cards added `kiln::app(id, name)` (conventions, a `Root` window
from `.root(..)`), `kiln_shell::Workspace<P>` (panes, docks, status bar, saved layout) and opt-in bundled keys,
menus and fail-fast on `Config`. They aren't joined up: an app still opens its own workspace window, makes its
`LayoutStore` (`examples/host/setup.rs:61-65`), calls `layout::launch` (`main.rs:39-44`) and registers its item
kinds (`main.rs:31`).

## Change

In `kiln/crates/kiln/src/app_builder.rs`:

- **The default window is a `Workspace<ItemPane>`** under `.key_context(..)` (default `"workspace"`), titled with
  the app's name. Builder methods: `.item::<T: PaneItem>()` (registers its kind for restore with
  `layout::register_item`), `.panel(DockPosition, |window, cx| -> Entity<impl DockItem>)`,
  `.status(fn(&App) -> Option<StatusItem>)` (a status source), `.dock_size(DockPosition, Pixels)` and
  `.workspace(|ws, window, cx| ..)`, a setup hook that runs once the window's workspace exists (to open the first
  items). `.root(..)` still gives a `Root` window instead. With neither `.root(..)` nor any item, panel or
  workspace hook, `start` panics with a message naming `.root(..)` and `.item::<T>()`.
- **Saved layout.** A `LayoutStore` over `AppPaths::layouts_dir()` with the state directory standing for the
  root (as the host example does); `layout::launch` opens a window for every saved entry no live window holds,
  else one fresh window; each workspace gets `set_layout` and, when there's a saved entry, `restore`.
  `.restore_layout(false)` opens one fresh window and saves nothing. A `.root(..)` window saves no layout.
- **Defaults on.** The builder's `Config` gets `default_keys(true)`, `default_menus(true)` and
  `fail_fast(cfg!(debug_assertions))`; `.config(..)` can turn each off.

Update `kiln::app`'s rustdoc table of conventions. New tests go in `kiln/crates/kiln/tests/app_workspace.rs`.

## Implementation notes

- `Started::window` is now an `AppWindow` (an `AnyWindowHandle` with `root::<V>(cx)`, `downcast` and `update`),
  so it can hold a `Workspace` or a `Root` window and `tests/app_builder.rs` still compiles unedited;
  `Started::windows` lists every window a restoring launch opened.
- Each workspace window is set up in a fixed order whatever the builder calls' order: panels, dock sizes,
  status sources, then the `.workspace(..)` hooks, then `restore` of the saved entry (its panes replace any the
  hooks opened) and `set_layout`.
- `.status(..)` alone doesn't make a workspace: the panic needs no root and no item, panel or workspace hook.
