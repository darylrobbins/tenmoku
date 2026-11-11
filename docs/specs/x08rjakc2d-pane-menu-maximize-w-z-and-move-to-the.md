---
task_uid: "01m74dvwedhym39kx08rjakc2d"
task: "T346"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p kiln-shell -E 'test(pane_zoom_exchange::)' passes: pane_zoom_exchange::a_zoomed_pane_is_drawn_alone (only it is laid out, over the group's whole area; unzooming lays every pane out again at its ratios), pane_zoom_exchange::a_split_a_close_or_another_active_pane_ends_the_zoom, pane_zoom_exchange::neighbours_while_zoomed_follow_the_layout_before_it"
    required: true
    manual: false
  - id: "C2"
    text: "pane_zoom_exchange::exchange_swaps_with_the_next_sibling_or_the_previous_when_last passes (members and ratios swap together, as nvim 0.12.6's \u003cC-w\u003ex; the other pane, now in the active pane's place, is active), and pane_zoom_exchange::exchange_with_a_split_sibling_or_alone_does_nothing passes"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in tests::pane_maximize (cargo nextest run -p fe-workbench) pass: ctrl-w z in vim normal mode and secondary-k secondary-m toggle the zoom, a lone pane does nothing, a focus key while zoomed ends it and lands where it would have unzoomed, ctrl-w x exchanges with focus kept in place; cargo nextest run -p fe-keymap --test vim_window_keys passes with z, x and ctrl-x in TABLE"
    required: true
    manual: false
  - id: "C4"
    text: "tests::pane_controls_b3::pane_menu_lists_the_design_actions_with_their_keys passes updated: Split Right, Split Down, Maximize, Move to the Other Split, Pin Tab, Close Other Tabs, Reopen Closed Tab, each with its keymap key; tests::pane_maximize::the_menu_row_reads_restore_while_zoomed passes; cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist, every_command_has_its_key)"
    required: true
    manual: false
---

# Pane menu: Maximize (⌃w z) and Move to the Other Split (⌃w x): pane.toggle_maximize and pane.exchange in kiln-shell's pane group, their keys and ⋯ rows (Mac T1723)

# Pane menu: Maximize (⌃w z) and Move to the Other Split (⌃w x)

Spec: `docs/design/design-system.md#workbench-anatomy`, Pane controls: the ⋯
menu offers "split right ⌃w v, split down ⌃w s, maximize ⌃w z, move to new
window, move to the other split ⌃w x, pin tab, …". T1542 left out maximize and
move to the other split because neither command existed; this card adds both
commands, their keys and their two menu rows. "Move to new window" is T1606's.

## Today

- The pane tree is kiln-shell's `PaneGroup` (`kiln/crates/kiln-shell/src/pane_group.rs`),
  held and drawn by `Workspace` (`kiln/crates/kiln-shell/src/workspace.rs`,
  `Panes::render`); fe-workbench wraps it (`crates/fe-workbench/src/pane_group.rs`)
  and runs the pane commands from `crates/fe-workbench/src/workbench_commands.rs`
  through helpers in `workbench_chrome.rs` (`focus_pane`, `split`, `activate`).
- Vim's `<C-w>` keys are plain keymap chords in `crates/fe-keymap/keymaps/fastedit.toml`
  (context `editor && vim.mode == normal`), pinned by
  `crates/fe-keymap/tests/vim_window_keys.rs`'s `TABLE`. fe-vim has no window
  key handling of its own (its `:sp`/`:vs`/`:on`/`:clo` go through
  `crates/fe-workbench/src/vim_windows.rs`). Nothing binds `<C-w>z` or `<C-w>x`.
- The LazyVim preset (`crates/fastedit/presets/lazyvim.vimrc`) lists
  `<leader>wm  zoom` as unmapped for want of a command.
- Layouts persist through `kiln/crates/kiln-shell/src/layout.rs`: `save_panes`
  writes `{tree, active}` (the active pane by its path), `shape()` decides
  when a change is saved, `restore_panes_with` gives `RestoredPanes {root,
  active, filled}`, applied in `Workspace`'s restore.

## Change

### Maximize: `pane.toggle_maximize`

- Zoom state lives in `PaneGroup`: at most one zoomed pane, always the
  workspace's active pane. While a pane is zoomed the group draws that pane
  alone, filling the group's area (no dividers); the other panes keep their
  items and ratios and are drawn again, as they were, when the zoom ends.
  Docks and the bottom panel are unchanged (VS Code's "maximize editor group",
  vim zoom plugins): this is not JetBrains' hide-all-tool-windows.
- `pane.toggle_maximize` zooms the active pane, or ends the zoom. With one pane
  it does nothing.
- The zoom ends when the tree changes (a split, a pane closed or removed, `:on`,
  an exchange, a restore's `set_root`) or another pane is made active (a focus
  key, `pane.focus_next`, a tab moved to another pane). Focus moves while zoomed
  go where they would have gone before the zoom: `PaneGroup::neighbour` uses the
  zoomed pane's bounds from before the zoom (record them when zooming), not the
  full-area bounds of the zoomed frame. Resize keys do nothing while zoomed;
  `pane.equalize` evens the hidden ratios and keeps the zoom.
- Keys: `ctrl-w z` in vim normal mode (beside the other `<C-w>` bindings), and
  VS Code's `secondary-k secondary-m` in the `workbench` context
  (`workbench.action.toggleMaximizeEditorGroup`; `secondary-k m` is taken by the
  language mode picker, `secondary-k secondary-m` is free). No LazyVim preset change here.
- Not kept across a restart (lead's call): `layout.rs` is unchanged and a restored layout is unzoomed. Because the zoom ends when the tree changes, a restore's `set_root` ends it too.

### Move to the other split: `pane.exchange`

nvim's `<C-w>x`, as nvim 0.12.6 does it (checked headless): the active pane
trades places with the next pane in its own row or column, or with the previous
one when it's the last there. Only a sibling that is a pane: if that sibling is
a split (an axis), or the pane is alone, nothing happens. Sizes travel with the
panes (nvim: a 30-column window moved right stays 30 columns), so the members
and their ratios swap together. Afterwards the pane now in the active pane's old
place, the other one, is active and focused ("the cursor is put in the other
window"): focus stays where it was on screen. No count (`3<C-w>x`).

- Keys: `ctrl-w x` and `ctrl-w ctrl-x` in vim normal mode. No non-vim key (VS
  Code has none).

### Commands, titles and the ⋯ menu

- Both are typed commands in `workbench_commands.rs` (context `workbench`), with
  `commands.ftl` titles: `command-pane-toggle_maximize = Toggle Maximized Pane`,
  `command-pane-exchange = Exchange with Next Pane` (worker may refine; the doc
  comment and the message must match byte for byte).
- The logic is kiln-shell's (`PaneGroup::{zoom, unzoom, zoomed, exchange}`, and
  `Workspace` methods that apply them to the active pane and clear the zoom where
  it sets `active` to another pane); fe-workbench's commands call them and
  `Workbench::activate` the pane an exchange makes active.
- `PANE_COMMANDS` (`crates/fe-workbench/src/toolbar_picker.rs`): after Split
  Down, "Maximize" (`pane.toggle_maximize`; reads "Restore" while the pane is
  zoomed, as Pin Tab reads Unpin Tab while pinned) and "Move to the Other Split"
  (`pane.exchange`), each with the key the active keymap binds in the menu's
  context, as T1542's rows. If T1606 lands first, its Move to New Window sits
  between them (the design's order). Titles in `l10n/en-US/pane.ftl`
  (`workbench-pane-action-maximize`, `-restore`, `-exchange`).
- Update T1542's `tests::pane_controls_b3::pane_menu_lists_the_design_actions_with_their_keys`
  to the new order.

## Tests

- kiln-shell: a new `kiln/crates/kiln-shell/tests/pane_zoom_exchange.rs` (the
  crate's tests are one binary, `tests/it.rs`, listed by build.rs: a new file is
  a new module, IDs `pane_zoom_exchange::<test>`), over `support::note` panes as
  `tests/pane_group.rs` does.
- fe-workbench: a new `crates/fe-workbench/src/tests/pane_maximize.rs`, declared
  from `crates/fe-workbench/src/tests/mod.rs` (IDs `tests::pane_maximize::<test>`).
  Placement matters: tests go in a `tests/**` file declared from a `tests/**/mod.rs`,
  not in a module declared from a non-test `src` file.
- fe-keymap: add `z`, `x` and `ctrl-x` to `crates/fe-keymap/tests/vim_window_keys.rs`'s
  `TABLE` (normal mode only, not insert/replace/vim off).
- No screenshot goldens change: nothing is drawn while unzoomed that wasn't.
  linux-ok.

## Out of scope

- Move to New Window (T1606), the app menu bar's Window menu
  (`crates/fe-keymap/menus/fastedit.toml`), a visible "zoomed" cue in the tab
  strip or status bar, hiding docks, a `<C-w>x` count, keeping the zoom across a restart, and the LazyVim `<leader>wm` mapping (a later card, once this lands).

## Implementation notes

- `PaneGroup` holds `zoomed: Option<(pane, bounds before the zoom)>`.
  `split`, `remove`, `set_root` and `exchange` clear it; `resize` refuses
  while zoomed; `equalize` keeps it. `neighbour` and `nearest` read the
  zoomed pane's recorded bounds (the hidden panes aren't laid out, so their
  own bounds are still the pre-zoom ones). `render` draws the zoomed pane
  alone in the group's container.
- `Workspace::set_active` ends the zoom when the active pane changes
  (`activate`, `move_tab`, a pane's click); `Workspace::toggle_zoom` and
  `Workspace::exchange` (which activates the other pane) are what the
  workbench's `pane.toggle_maximize` and `pane.exchange` call.
- An exchange that moves nothing (a split sibling, a lone pane) keeps the zoom.
- Command titles as proposed: Toggle Maximized Pane, Exchange with Next Pane.
- Out of scope, touched: `crates/fe-workbench/src/workbench_palettes.rs`
  (`open_toolbar_picker` sets the picker's `zoomed` flag: the picker opens
  inside the workbench's update, so it can't read the workbench for the
  zoom); `crates/fe-workbench/src/tests/pane_toolbar.rs`
  (`buttons_and_pane_commands` lists the two new rows) and
  `crates/fe-gpui/src/vim/which_key_chord_tests.rs`
  (`the_wheel_scrolls_the_popup_not_the_text`: `<C-w>`'s last key is now `z`,
  not `W`).
