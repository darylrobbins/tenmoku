---
task_uid: "01m493bzn0nmth647vw7c4mty4"
task: "T128"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::title_bar::off_by_default: a Workbench whose host never calls set_title_bar draws no title-bar selector, and its first pane's tab bar starts at y = 0, as before"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::title_bar::shows_the_root_name: after set_title_bar(true) with root \u003ctmp\u003e/demo, title-bar spans the window's width at y = 0 and is TITLE_BAR_HEIGHT (32 px) high; title-bar-label reads demo and its centre is the window's horizontal centre within 1 px; the panes, dock and rail start at y = 32 and the status bar still ends at the window's bottom; with no root it reads fastedit; after set_root to another folder it reads that folder's name; in a 300 px wide window a 100-char root name's label lies within TRAFFIC_LIGHT_INSET of each edge"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::title_bar::drag_and_double_click: a left mouse down with click_count 1 on the bar's empty space, and one on the label, each record one window-move request in the test-only record; a mouse down with click_count 2 records one double-click request and no move; mouse downs on the tab bar and in the editor record neither"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fastedit/src/main.rs opens its window with titlebar Some(fe_workbench::titlebar_options(\"fastedit\")) (appears_transparent true, traffic_light_position Some(TRAFFIC_LIGHTS)) and app_owns_titlebar_drag true, and calls set_title_bar(true) before the first frame; no other crate sets TitlebarOptions; existing tests::tabs, tests::status_bar and tests::activity_rail tests pass unchanged. The look and feel in the running app (traffic lights centred in the bar, dragging, double-click) is checked by the user after landing, in a separate action task, not here"
    required: true
    manual: false
---

# Unify the standalone app's macOS title bar: traffic lights inline, the project name centred

# Unify the standalone app's macOS title bar: traffic lights inline, the project name centred

Workbench polish asked for by the user: the standalone app's window gets a transparent, unified title
bar, drawn by the workbench, with the traffic lights inline, the project's root folder name centred, and
the window still draggable from the bar's empty space. Only `crates/fastedit` opts in; an embedding host
keeps its own chrome. Keep it small.

It builds on:

- T63's `Workbench`, T123's `Workbench::set_root` / `RootChanged`, and T125's root column (the existing
  row, then the status bar)
- `crates/fastedit/src/main.rs`'s `open_window`, which passes `TitlebarOptions { title, .. }` today

## What gpui-pre 0.3.7 offers (checked in its Apache-2.0 source)

- `TitlebarOptions { title, appears_transparent, traffic_light_position }`. On macOS,
  `appears_transparent` adds `NSFullSizeContentViewWindowMask`, sets `titlebarAppearsTransparent` and
  hides the title text, so the content view draws from the window's top edge, under the traffic lights.
  `traffic_light_position` is the buttons' offset from the top-left, re-applied on resize and full
  screen changes.
- `WindowOptions::app_owns_titlebar_drag` (macOS only): the whole content view is reported as app-owned
  titlebar content, so AppKit neither drags the window from the top strip nor delays clicks there while
  it waits to tell a double-click from a click. The app then moves the window itself with
  `Window::start_window_move` (`performWindowDragWithEvent:` with the current event).
- `Window::titlebar_double_click()` applies the user's System Settings choice
  (`AppleActionOnDoubleClick`: zoom / fill, minimize, or nothing).
- `window_control_area(WindowControlArea::Drag)` is a no-op on macOS (`on_hit_test_window_control` is
  empty there), so it isn't used.
- The test platform's `start_window_move` is `unimplemented!()` and its `titlebar_double_click` does
  nothing, so tests observe both through a test hook (below), never by calling them.

## Change

### `crates/fe-workbench/src/title_bar.rs` (new)

- `pub const TITLE_BAR_HEIGHT: Pixels = px(32.)`, `pub const TRAFFIC_LIGHTS: Point<Pixels>` (the
  buttons' position, `point(px(12.), px(9.))` so they sit centred in the bar; tune by eye against the
  running app and note the value in a comment) and `TRAFFIC_LIGHT_INSET` (the width the buttons take,
  plus a gap: about 78 px).
- `pub fn titlebar_options(title: impl Into<SharedString>) -> TitlebarOptions`: `appears_transparent:
  true`, `traffic_light_position: Some(TRAFFIC_LIGHTS)`. Its doc says a host opting in must also set
  `WindowOptions::app_owns_titlebar_drag: true`, and call `Workbench::set_title_bar(true)`.
- The bar: full window width, `TITLE_BAR_HEIGHT` high, `panel_background`, 1 px bottom `border`. The
  label is the root folder's name in `text` colour, centred on the **window's** width (as macOS centres
  titles), single line, ellipsized so it never enters `TRAFFIC_LIGHT_INSET` on either side. With no
  root it reads `fastedit`. It follows the workbench's current root, so `set_root` retitles it.
- **Drag and double-click:** a left mouse down anywhere on the bar (the label isn't interactive) with
  `click_count == 1` calls `window.start_window_move()`; with `click_count == 2` it calls
  `window.titlebar_double_click()` and doesn't start a move. Under `cfg(test)` both go to a test-only
  record instead. Nothing below the bar moves the window.
- Debug selectors `title-bar` and `title-bar-label`.

### `crates/fe-workbench/src/workbench.rs`

- `pub fn set_title_bar(&mut self, shown: bool, cx)`; off by default, so a host that doesn't call it
  sees no change.
- When shown, the root column gets the bar first, above T125's row (panes, dock, rail) and status bar.
  Palettes still overlay the whole window.
- `window.set_window_title` stays as it is: AppKit hides the title text, but the Window menu, Mission
  Control and the Dock still read it.

`crates/fe-workbench/src/lib.rs`: a `mod` line and `pub use` of the title bar's public items.

### `crates/fastedit/src/main.rs`

`open_window` gets `titlebar: Some(fe_workbench::titlebar_options("fastedit"))` and
`app_owns_titlebar_drag: true`, and the workbench is built with `set_title_bar(true)` before the first
frame. Nothing else changes.

### Tests

New file `crates/fe-workbench/src/tests/title_bar.rs`, declared in `tests/mod.rs`: headless GPUI,
`debug_bounds` on the selectors, mouse downs sent with `simulate_event(MouseDownEvent { click_count, .. })`.

## Out of scope

- Buttons, menus or tabs in the title bar; dragging the window from the tab bar's empty space.
- Linux and Windows decorations (`window_decorations`, client-side resize edges).
- Dimming the bar or its label while the window is inactive.
- Hiding the bar in full screen.
- Showing the active file in the bar (the tab bar and breadcrumbs show it; the window title still names
  it).

## Decided (lead defaults; the user may revisit)

- **The project name is centred**, as macOS centres window titles and VS Code its title, rather than
  left after the traffic lights.
- **The app owns the drag** (`app_owns_titlebar_drag` plus `start_window_move`) rather than AppKit's
  native strip, which covers only AppKit's 28 px titlebar height and delays clicks in it.
- The bar is drawn by the workbench, opted in by the host, so `crates/fastedit` and the workbench shell
  change and an embedding host doesn't.
- 32 px high.
