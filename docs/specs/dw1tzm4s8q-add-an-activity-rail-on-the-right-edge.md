---
task_uid: "01m490cq6gdycae5dw1tzm4s8q"
task: "T120"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::activity_rail::the_rail_stays_when_the_dock_hides: the rail's laid-out bounds end at the window's right edge and are RAIL_WIDTH (36 px) wide, with the 240 px dock to their left; after cmd-b the rail's bounds are unchanged and the pane's right edge moves right by 240 px"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::activity_rail::clicks_switch_and_collapse: with Project shown, clicking the Search button shows Search with its input focused; clicking Search again hides the dock and focuses the active editor; clicking Outline while hidden shows Outline with focus left in the editor; clicking Project shows Project; each click runs the DockOp of that panel's command, or Toggle (a test-only record of Dock::run's ops)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::activity_rail::only_the_shown_panel_is_highlighted: Dock::rail_highlighted() is Some(Search) after cmd-shift-f, None after cmd-b, and Some(Outline) after cmd-shift-o; the dock's title row reads the active panel's name and clicking it changes nothing; the dock renders no TabBar"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::activity_rail::tooltips_show_name_and_shortcut: hovering a button and advancing the clock 500 ms records the tooltips Project ⇧⌘E, Search ⇧⌘F and Outline ⇧⌘O; after fe_gpui::set_user_keymap with a user layer binding search.show to alt-cmd-f, the Search tooltip reads Search ⌥⌘F; with cmd-shift-f unbound and no other binding for search.show it reads Search"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::activity_rail::buttons_draw_lucide_icons: the rail's buttons draw Icon::Files, Icon::Search and Icon::ListTree in that order (Dock::rail_icons()), and fe_ui::Assets loads each one's path; this card changes no file under crates/fe-ui/assets and no licence file"
    required: true
    manual: false
  - id: "C6"
    text: "fe-keymap test tests/label.rs::macos_labels: macos_label gives ⇧⌘F for cmd-shift-f, ⌃⌥⌘← for ctrl-alt-cmd-left, ⌘K ⌘W for cmd-k cmd-w, ⌥Z for alt-z, F12 for f12, ⌘\\ for cmd-\\, and ↩ ⎋ ⌫ ⌦ ⇥ Space ⇞ ⇟ ↖ ↘ for enter, escape, backspace, delete, tab, space, pageup, pagedown, home and end"
    required: true
    manual: false
  - id: "C7"
    text: "Existing tests::search_panel and tests::outline_panel tests pass, with header-tab clicks replaced by rail clicks and no other change to those files; no new commands or bindings; rg 'KeyBinding::new|actions!' finds no match in the files this card adds"
    required: true
    manual: false
---

# Add an activity rail on the right edge to switch and collapse the dock's panels

# Add an activity rail on the right edge to switch and collapse the dock's panels

Milestone M3 "Browse", after the outline, menu bar and pane-drag cards. It builds on:

- T77's `Dock`, `DockPanel`, `DockOp` and `Dock::run`, and its commands: `search.show`
  (cmd-shift-f), `dock.show_project` (cmd-shift-e) and `dock.toggle` (cmd-b)
- T78's Outline panel and `outline.show` (cmd-shift-o)
- T111's fix that moves focus off a hidden panel's input when switching by click
- the menu-bar card's `fe_gpui::current_keymap` and `fe_keymap::menu::shortcuts`
- T115's Lucide icons, `fe_ui::Icon` and `fe_ui::Assets`, and T116's tree restyle, which shares
  `theme.rs`

## Problem

The dock switches panels through a header strip of names (fe-ui `TabBar`) inside the dock. Once the dock
is hidden (cmd-b), nothing on screen brings it back, and nothing shows which key opens which panel. With
Outline added, and References coming in M4, the 240 px strip is crowded and starts to scroll.

## Change

### The rail

New file `crates/fe-ui/src/activity_rail.rs`: `ActivityRail` and `RailButton`, stateless `RenderOnce`
elements, with `pub const RAIL_WIDTH: Pixels = px(36.)`. `crates/fe-ui/src/lib.rs` gets `mod` and
`pub use` lines.

- The rail is a full-height vertical strip at the window's right edge, outside the dock: the layout is
  panes, then the dock, then the rail. It shows whether or not the dock does.
- It has a `panel_background` fill and a 1 px `border` line on its left.
- **One button per dock panel, in the dock's order:** Project, Search, Outline. The list comes from
  `Dock`'s own panel list, so a later panel (M4's References) adds one entry there.
- **Buttons:** 36 × 36 px with the icon centred, `row_hover` on hover, and
  `CursorStyle::PointingHand`.
- **The highlight:** the active panel's button is highlighted only while the dock is visible. Its icon
  is in `tab_active_text`, and a 2 px bar in a new `UiTheme::accent` colour runs down the rail's left
  edge, facing the dock. Other icons are in T116's `icon` colour. While the dock is hidden, no button
  is highlighted.
  - `crates/fe-ui/src/theme.rs` gets one field, `accent` (`default_dark` `0x4f7cc8`).

### Icons

The rail uses T115's Lucide set and its `fe_ui::Assets` source. T115 bundles the rail's three icons,
with their licence records:

- `Icon::Files` (Project)
- `Icon::Search` (Search)
- `Icon::ListTree` (Outline)

This card adds no SVG files and no licence rows.

- `RailButton::new(id, icon: fe_ui::Icon, …)` draws `svg().path(icon.path())` at 18 px, a monochrome
  mask tinted with the text colour.
- `dock.rs` maps each panel to its icon in one function, `panel_icon(DockPanel) -> Icon`.

### Clicking

- **A different panel's button** shows the dock with that panel. It runs the same `DockOp` as that
  panel's show command, with the same focus behaviour:
  - Search focuses its input.
  - Project behaves as `dock.show_project` does.
  - Outline leaves focus in the editor.
- **The active panel's button, while the dock is visible,** hides the dock: `DockOp::Toggle`, including
  moving focus off a hidden input.
- **Any button while the dock is hidden** shows the dock with that panel.
- **One code path.** The rail sits inside `Dock`'s render. A click emits `DockEvent::Rail(DockPanel)`.
  The workbench subscribes to it in `crates/fe-workbench/src/workbench.rs` (one subscription) and calls
  `Dock::run` with its fallback focus, as `on_run_command` does for keys. A click and a key therefore
  run the same op.

### Tooltips

New file `crates/fe-ui/src/tooltip.rs`: `Tooltip`, a small view showing a label and, if there is one, a
muted shortcut. It's used through GPUI's `.tooltip(…)`, with the default 500 ms delay.

- Each button's tooltip is the panel's name and the shortcut of its show command (`dock.show_project`,
  `search.show` or `outline.show`).
- The shortcut is the first entry of `fe_keymap::menu::shortcuts(current_keymap, command, {},
  [workbench, editor])`, printed with macOS symbols, for example `Search ⇧⌘F`. With no binding, it's
  the name alone.
- It's read when the tooltip is built, so it follows keymap hot reload with no extra wiring.

New file `crates/fe-keymap/src/label.rs`: `pub fn macos_label(keys: &KeySequence) -> String`.
`crates/fe-keymap/src/lib.rs` gets `mod` and `pub use` lines.

- Modifiers go in the order ⌃⌥⇧⌘, followed by the key.
- Letters are uppercase.
- The arrows are ← → ↑ ↓.
- The named keys are: enter ↩, escape ⎋, backspace ⌫, delete ⌦, tab ⇥, space `Space`, pageup ⇞,
  pagedown ⇟, home ↖ and end ↘.
- Function keys are `F1` and so on.
- Other characters print as themselves.
- The keystrokes of a chord are separated by a space.

### The dock's header

The rail **replaces** the header strip. In `crates/fe-workbench/src/dock.rs`, the `TabBar` of panel
names goes, and so do its `ScrollHandle` and `Dock::tab_bounds`. In its place is a non-interactive title
row, `TAB_HEIGHT` high so it lines up with the pane's tab bar, showing the active panel's name in
`muted_text`.

Why it replaces rather than complements:

- Two switchers for the same state are redundant.
- The strip would scroll at 240 px once Outline and References join.
- The rail is the one that survives hiding the dock.

The title row keeps the dock saying what it shows.

`Dock::rail_button_bounds(DockPanel)` (test-only) replaces `tab_bounds`. The existing tests that clicked
a header tab click the rail instead. That changes only those lines, in
`crates/fe-workbench/src/tests/search_panel.rs`, and in `crates/fe-workbench/src/tests/outline_panel.rs`
if T78's tests click header tabs.

### Commands

There are no new commands or bindings. Clicks run the existing dock ops.

### Tests

- New file `crates/fe-workbench/src/tests/activity_rail.rs`, declared in `tests/mod.rs`. It uses
  headless GPUI and clicks laid-out bounds, as T77's tests do.
- A test-only hook records the last tooltip's text.
- New file `crates/fe-keymap/tests/label.rs`.

## Out of scope

- Reordering, hiding or adding panels from the rail.
- Badges, such as a search result count.
- A left-side rail, or moving the dock to the left.
- A resizable dock.
- Context menus on rail buttons.
- Choosing or bundling icons: T115 does that.

## Decided (lead defaults; the user may revisit)

- **The rail replaces the header strip.** The dock keeps a plain title row.
- The rail is at the far right, outside the dock, as VS Code's activity bar is when its side bar is on
  the right. It is 36 px wide.
- Clicking the active panel collapses the dock. Clicking another switches to it, showing the dock.
  While the dock is hidden, nothing is highlighted.
- Tooltips show the name and the shortcut in macOS symbols.
