---
task_uid: "01m493af7bctsgymyec9hp6a8s"
task: "T127"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::pane_toolbar::layout_and_buttons: with src/util/mod.rs open, the pane draws its tab bar, then a TOOLBAR_HEIGHT (24 px) toolbar spanning the pane's width directly below it, then the editor below that; the crumbs read src, util, mod.rs (crumb:\u003cix\u003e:\u003clabel\u003e selectors), the file crumb carrying file_icon(mod.rs); back and forward are drawn disabled and clicking them dispatches nothing; Split Right, Split Down and … sit at the tab bar's right end and keep the same bounds after 30 more tabs open and the bar scrolls"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::pane_toolbar::symbols_follow_the_caret: once the outline lands, the caret inside fn b's body gives symbol crumbs outer, impl S, b; moving it into fn main gives main; onto the blank line between items, none; with the dock hidden or showing Project the crumbs still follow; the toolbar starts no outline job of its own (OutlinePanel::jobs_started unchanged by 20 caret moves) and its chain recomputation counter rises once per caret move or landed outline, never on a redraw alone"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::pane_toolbar::path_crumbs_open_entry_pickers: clicking the util crumb shows the toolbar picker listing util's entries, directories first, each sorted case-insensitively, read off the main thread; clicking the mod.rs crumb lists the same with mod.rs selected; typing filters by subsequence with highlights; choosing a directory lists its entries with the query cleared; choosing a file opens it in the clicked pane, active and focused; escape dismisses it with focus back on the editor"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::pane_toolbar::symbol_crumbs_open_sibling_pickers: clicking the b crumb lists a and b with b selected; choosing a puts the caret at a's name start, focuses the editor and the crumbs read outer, impl S, a; when the outline's generation moves while the picker is open (an edit to the shared buffer from a split pane, then run_until_parked), a pick is refused and the list is rebuilt from the new outline"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::pane_toolbar::buttons_and_pane_commands: with two panes, clicking Split Right in the inactive pane activates it and splits it (three panes, as cmd-backslash from there), recording one pane.split_right invocation with source Mouse; Split Down likewise; … lists Close Tab, Close Pane, Split Right, Split Down, Move Tab Left/Right/Up/Down and Toggle Breadcrumbs with their shortcuts (Split Right shows ⌘\\, Close Pane none since it is a chord); choosing Close Pane closes the pane whose … was clicked; hovering Split Right for 500 ms records the tooltip Split Right ⌘\\; with NavButtons enabled by the test-only setter, clicking back dispatches nav.back with source Mouse"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::pane_toolbar::toggle_hides_every_toolbar: view.toggle_breadcrumbs dispatched in the workbench hides the toolbar in both panes (each editor's top moves up by TOOLBAR_HEIGHT) while the tab-bar buttons stay, a pane split while hidden has no toolbar, and toggling again restores both; while hidden the chain recomputation counter does not rise on caret moves"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::pane_toolbar::crumbs_per_pane_and_root: two panes showing different files each show their own path; the inactive pane keeps the symbol chain it showed when last active; a tab moved to the other pane (ctrl-cmd-right) shows its path there; a file outside the root shows its parent directory's name then its name; the welcome buffer shows untitled; after set_root to the file's own directory its crumbs are recomputed to the file name alone; in a 320 px wide pane a/b/c/d/e/f/a_long_file_name.rs keeps the file crumb wholly inside the toolbar's bounds"
    required: true
    manual: false
  - id: "C8"
    text: "view.toggle_breadcrumbs is registered through the command registry with context workbench and no default binding, known to fe_workbench::command_catalog(); the View menu in crates/fe-keymap/menus/default-macos.toml lists Toggle Breadcrumbs after Toggle Dock and fastedit's tests/menus.rs::default_menus_know_every_command passes; default-macos.toml keymap, command_tests.rs and keymap_tests.rs do not change; rg 'KeyBinding::new|actions!' finds no match in the files this card adds"
    required: true
    manual: false
  - id: "C9"
    text: "crates/fe-ui changes only by the new icon_button.rs and breadcrumbs.rs and mod/pub use lines in lib.rs; no file under crates/fe-ui/assets, THIRD_PARTY.md or NOTICE changes; outline_panel.rs changes only by one pub(crate) accessor; existing tests::tabs, tests::splits, tests::pane_drag, tests::outline_panel and tests::activity_rail tests pass unchanged"
    required: true
    manual: false
  - id: "C10"
    text: "Size: the non-test diff may exceed the 1,000-line merge cap, up to about 1,700 changed lines; the overage was accepted on 2026-10-07 instead of splitting the card, so review F3 (cap exceeded) is resolved by this amendment, not by moving the pane-commands list out"
    required: true
    manual: false
---

# Add a breadcrumb toolbar to each pane, with split and pane-menu buttons at the tab bar's end

# Add a breadcrumb toolbar to each pane, with split and pane-menu buttons at the tab bar's end

Workbench polish asked for by the user: a thin toolbar per pane, under its tab bar, with back and
forward, then breadcrumbs `dir › dir › file › Symbol › Symbol`. It builds on:

- T79's `PaneGroup` and active pane, its `pane.*` commands, and T119's tab moves between panes
- T78's `OutlinePanel`, which already keeps the active editor's `fe_syntax::Outline` current off the
  main thread (one job at a time, refreshed after the frame via `frame_drawn`), and T64's
  `OutlineItem { name, range, name_range, depth, parent }`, in pre-order by start
- T76's `Picker`/`PickerDelegate`, `PaletteFrame` and `HighlightedLabel`, hosted by the workbench as
  quick-open is
- T115's Lucide icons (`fe_ui::Icon`, `fe_ui::Assets`) and T120's `fe_ui::Tooltip` with
  `fe_keymap::macos_label` and `fe_keymap::menu::shortcuts` over `fe_gpui::current_keymap`
- T125's status bar card, which turns the workbench's quick-open-only palette flag into "which palette
  is open", and T128's title bar (both edit `workbench.rs` first)
- T117's menu table (`crates/fe-keymap/menus/default-macos.toml`) and T123's `Workbench::set_root` /
  `RootChanged`
- the command registry (T68)

It lands **before T81's golden screenshot**, so the golden shows the final pane chrome. T107 (back and
forward history) lands after T81, so this card draws the back and forward buttons in their final place,
**disabled**; the follow-up card that depends on T107 enables them. Nothing here depends on T107.

## Problem

Nothing in a pane says where the file sits in the project or where the caret is in the file, and the
splits are reachable only by key or menu.

## Change

### Layout (per pane, top to bottom)

1. **The tab bar row**: T63's `TabBar`, which scrolls, then a fixed **actions strip** at its right end:
   Split Right (`Icon::Columns2`), Split Down (`Icon::Rows2`) and a "…" button (`Icon::Ellipsis`).
   - `crates/fe-workbench/src/pane.rs` wraps the `TabBar` in a row: the bar in a `flex_1().min_w_0()`
     box, the strip `flex_none()` with the bar's background and bottom border, so it reads as one bar.
     `crates/fe-ui/src/tab_bar.rs` doesn't change (T119 owns its drag hooks).
   - The strip doesn't scroll with the tabs: with many tabs it stays put.
2. **The toolbar**, `TOOLBAR_HEIGHT` (24 px) high, `panel_background`, 1 px bottom `border`:
   back (`Icon::ArrowLeft`), forward (`Icon::ArrowRight`), then the breadcrumbs.
3. The editor, or the loading / error / empty placeholder, as now.

### `crates/fe-ui/src/icon_button.rs` (new)

`IconButton`, a stateless `RenderOnce`: a 16 px `svg()` icon in a 22 × 22 px box, `row_hover` on
hover, `CursorStyle::PointingHand`, an optional tooltip builder, and `.disabled(bool)`: a disabled
button draws its icon in `muted_text`, has no hover, and runs no click handler. Debug selector
`icon-button:<id>` (free outside test-support builds, as T116's are).

### `crates/fe-ui/src/breadcrumbs.rs` (new)

`Breadcrumbs`, a stateless `RenderOnce`, and `pub const TOOLBAR_HEIGHT: Pixels = px(24.)`.

- Segments `Crumb { label, icon: Option<Icon>, kind: Path | File | Symbol }` with a click handler each,
  separated by `Icon::ChevronRight` at 12 px in `muted_text`.
- The file segment carries `fe_ui::file_icon(name)`. Symbols have no icon (symbol-kind icons are out
  of scope).
- **Too narrow:** directory segments shrink first, each down to an ellipsis (`text_ellipsis`); the file
  and symbol segments keep their width and are clipped at the right edge last.
- Debug selectors `crumb:<index>:<label>`.

`crates/fe-ui/src/lib.rs`: `mod` and `pub use` lines only.

### What the crumbs show: `crates/fe-workbench/src/pane_toolbar.rs` (new)

- **Path:** the active tab's path relative to the workbench's current root, one segment per directory,
  then the file name. A file outside the root (T123's Open File…, T124's settings file) shows its parent
  directory's name, then the file name. An untitled buffer shows `untitled`. While a file is read, or
  its read failed, the crumbs show that file's path and no symbols. With no tab, the toolbar shows the
  buttons only.
- **Symbols:** the chain of outline items enclosing the caret (the primary selection's head), outermost
  first. With the T64 data it's a binary search for the last item starting at or before the caret, then
  a walk up `parent` links while the range contains it: O(log n + depth), no allocation beyond the chain.
  - **No outline work of its own.** The symbols come from the `Outline` that `OutlinePanel` already
    holds for the active editor (whether or not the dock shows it). `crates/fe-workbench/src/outline_panel.rs`
    gains one `pub(crate)` accessor: the outline shown for a given editor, with its generation. Nothing
    else in it changes.
  - The chain is recomputed only when the caret moves, the active editor changes, or the outline's
    generation moves, not on every frame. A test-only counter records recomputations.
  - Until the first outline lands, or for plain text, an unsupported language or a too-large file, no
    symbols show.
  - **Active pane only.** The outline follows the active pane's editor, so the workbench pushes the
    chain into the active pane as it follows the active editor (T78's `follow_active_editor` hook, and
    one `cx.observe` of the outline panel so a landed outline updates the crumbs). An inactive pane keeps
    the chain it showed when last active; its caret can't move while it's inactive.
- **Root changes:** on T123's `RootChanged`, every pane's path crumbs are recomputed.
- **Back and forward** are drawn **disabled** in this card. Their state comes from one seam,
  `pub(crate) struct NavButtons { back: bool, forward: bool }`, that the workbench pushes into each pane;
  this card always pushes both `false`. An enabled button dispatches `nav.back` or `nav.forward` as a
  `RunCommand` with `Source::Mouse`. A test-only setter enables them, to test that a click dispatches.
  The follow-up card wires the seam to T107's `NavHistory`.

### Pickers: `crates/fe-workbench/src/toolbar_picker.rs` (new)

One `PickerDelegate`, `ToolbarPicker`, hosted by the workbench in `PaletteFrame` exactly as quick-open
is: one more variant of T125's "which palette is open" state, with its dismiss and blur handled as
quick-open's are. It remembers the pane it was opened
from. Rows match the query by case-insensitive subsequence, highlighted with `HighlightedLabel`, in list
order (no re-sorting). Escape dismisses it and focus returns where it was. It has three lists:

- **Entries**, from a path segment:
  - Clicking a **directory** segment lists that directory's entries. Clicking the **file** segment lists
    its directory's entries with the file selected.
  - The directory is read with `std::fs::read_dir` on the background executor, never on the main thread.
    Symlinks aren't followed (a link shows as a file). Directories come first, then files, each sorted
    case-insensitively. Entries show their `file_icon` / `folder_icon`. A read error shows as the status
    line, with no rows.
  - Choosing a file opens it in the picker's pane through `Pane::open`, as a tree click does. Choosing
    a directory lists its entries, clearing the query.
  - The status line reads the directory's path relative to the root.
- **Symbols**, from a symbol segment: the clicked item's siblings (the items with the same `parent`), in
  outline order, the clicked one selected. Choosing one selects its name start with
  `select_and_reveal` and focuses that editor, as T78's outline click does. Each pick is checked against
  the outline generation it was listed from; if the outline changed since, the list is rebuilt from the
  current outline for the same crumb (by its chain of names) and the pick is refused, so a pick never
  lands on a moved symbol. If the chain no longer exists the status reads "No symbols".
- **Pane commands**, from the "…" button: Close Tab (`workbench.close_tab`), Close Pane (`pane.close`),
  Split Right, Split Down, Move Tab Left/Right/Up/Down (`pane.move_tab_*`) and Toggle Breadcrumbs
  (`view.toggle_breadcrumbs`). Each row shows the command's title and its shortcut
  (`macos_label` of the first `shortcuts` entry in `[workbench, editor]`, as T120's tooltips read it;
  none when unbound). Choosing one activates the picker's pane, then dispatches the command as a key
  would (`RunCommand`, `Source::Palette`) so the same handler runs.

### Buttons

- Split Right and Split Down activate their pane (T79's mouse-down already does) and dispatch
  `pane.split_right` / `pane.split_down` as a `RunCommand` with `Source::Mouse`: the same handler as
  `cmd-\` and `cmd-k cmd-\`.
- Every button has a T120 `Tooltip`: its title and shortcut, e.g. `Split Right ⌘\`, `Go Back`
  (T107 adds `nav.back`'s key, which the tooltip then shows with no change here).

### Hiding the toolbar

New command `view.toggle_breadcrumbs`, "Toggle Breadcrumbs", context `workbench`, **no default
binding**, registered through the registry (in `crates/fe-workbench/src/lib.rs`'s registries, known to
`command_catalog()`). It hides or shows the toolbar row in every pane, including panes split later; the
actions strip in the tab bar stays. While hidden, no symbol chain is computed. The state isn't persisted
(a settings key is out of scope).

`crates/fe-keymap/menus/default-macos.toml`: View gets "Toggle Breadcrumbs" (`view.toggle_breadcrumbs`)
after Toggle Dock.

### Wiring

- `crates/fe-workbench/src/workbench.rs`: hosting the toolbar picker, pushing the symbol chain and
  `NavButtons`, the toggle, and `RootChanged`. Keep it to small hunks.
- `crates/fe-workbench/src/pane.rs`: the actions strip and the toolbar row, the per-pane crumb state.
- `crates/fe-workbench/src/pane_group.rs`: only if it lacks a way to reach every pane.
- `crates/fe-workbench/src/lib.rs`: `mod` lines, the command's registry, and its place in
  `command_catalog()`'s chain.

### Icons

This card uses `Icon::ArrowLeft`, `Icon::ArrowRight`, `Icon::Columns2`, `Icon::Rows2`, `Icon::Ellipsis`
(T115's list, extended by the lead for this card) and T115's `Icon::ChevronRight`. It adds no SVG file
and no licence row.

### Tests

New file `crates/fe-workbench/src/tests/pane_toolbar.rs`, declared in `tests/mod.rs`: headless GPUI
over a `tempfile` fixture, clicks at laid-out bounds (`debug_bounds` on the selectors above), outline
jobs run with `run_until_parked`. The fixture's `src/util/mod.rs` is Rust:

```rust
mod outer {
    pub struct S;
    impl S {
        fn a() {}
        fn b() {
            let x = 1;
        }
    }
}
fn main() {}
```

The merge cap is normally 1,000 changed lines, not counting tests. For this card the overage is
accepted (2026-10-07, after review round 1, on the user's instruction to unstick blocked tasks): the
non-test diff of about 1,600 lines, with the entry, symbol and "…" pane-command lists all kept, lands
as one card. No split is wanted.

## Out of scope

- Enabling back and forward (the follow-up card, after T107).
- A dropdown anchored under the clicked crumb: the pickers use the palette's place, as quick-open does.
- Symbol-kind icons in crumbs or the symbol picker.
- Keyboard focus of the breadcrumbs (VS Code's "focus breadcrumbs").
- Persisting the toggle; a `breadcrumbs` setting.
- Showing ignored-file styling, or hiding ignored entries, in the entries picker.
- Outlines for inactive panes' editors.

## Decided (lead defaults; the user may revisit)

- **Split and "…" buttons sit at the right end of the tab bar**, not on the toolbar: that's where VS
  Code puts editor actions; they stay visible when the breadcrumbs are hidden; and the toolbar's width
  stays for the crumbs, which are long.
- **Back and forward are drawn now, disabled,** so T81's golden needs no regeneration when T107 lands.
- Symbols come from T78's cached outline for the active pane; inactive panes keep their last chain.
- Clicking a directory crumb lists that directory; clicking the file crumb lists its siblings; clicking
  a symbol lists its siblings. Every list is a T76 picker.
- `view.toggle_breadcrumbs` has no default key, as in VS Code; it's in the View menu.
