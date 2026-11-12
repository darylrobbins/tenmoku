---
task_uid: "01m78dvbcmbwyf1805tnaka19x"
task: "T1750"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::project_menu::right_click_selects_row_and_opens_menu, folder_menu_lists_new_and_delete, file_menu_has_no_new_items, empty_space_menu_offers_new_and_paste, menu_items_run_the_panel_commands, escape_and_outside_click_close_it, arrows_and_enter_choose_an_item pass"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_ops_tests, project_panel_clipboard_tests, project_panel_nav_tests, tests::sidebar_b3, tests::project_panel and a11y_tabs_tree pass unchanged; cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden unchanged and the new project_tree_menu audit scene was looked at"
    required: true
    manual: false
---

# Project tree: a right-click menu on every row exposing New File, New Folder, Rename, Duplicate, Delete, Cut, Copy, Paste, Copy Path and Collapse All

# Project tree: a right-click menu on every row

Today the tree has no context menu (docs/decisions/qenw9xxdzk-accessibility-design.md: "every operation is a project_panel.* command, and it has no context menu"). New File, New Folder, Rename, Delete (to the Trash, folders included), Duplicate, Cut, Copy, Paste, Copy Path and Copy Relative Path all exist as `project_panel.*` commands (project_panel.rs `registry()`) and keys, so the menu only exposes them.

## Change
- kiln-ui `TreeRow` (kiln/crates/kiln-ui/src/tree_row.rs) gets `on_secondary_click(position, ...)`.
- New `crates/fe-workbench/src/project_panel_menu.rs`: a right click selects the row (not clicking through to open or toggle it) and draws kiln-ui `MenuPanel` in a deferred, anchored element at the click, closed by Escape, a click outside or a refresh, with Up/Down/Enter as git_log_menu.rs does. Items are the panel's own commands, run through `run_op`: on a folder: New File, New Folder, separator, Rename, Duplicate, Delete, separator, Cut, Copy, Paste, separator, Copy Path, Copy Relative Path, separator, Collapse All; on a file: the same minus New File/New Folder/Paste-into (paste goes beside it, as today); on empty space below the rows: New File, New Folder, Paste, Refresh. Shortcuts shown right-aligned from the keymap. Items that do nothing now are absent, not disabled (Paste with nothing cut or copied).
- Strings in `crates/fe-workbench/l10n/en-US/project.ftl` (`workbench-project-menu-*`), command titles reused.
- A mouse right-click while a name field is open commits nothing and opens nothing.

## Pins (must pass unchanged)
project_panel_ops_tests, project_panel_clipboard_tests, project_panel_nav_tests, project_panel_hover_tests, tests::sidebar_b3, tests::project_panel, a11y_tabs_tree.rs, a11y_reach_panels.rs. No existing golden moves: the menu is only drawn after a right click; add the audit scene `audit/project_tree_menu.rs` (declared from audit/mod.rs).

Platform: Mac-only for the audit scene only; the rest is linux-ok.

## Implementation notes

- Titles are the commands' own (`command-project_panel-*`), so Delete reads "Move to Trash" and Collapse All "Collapse All Folders", as the palette lists them. The one new string, `workbench-project-menu`, names the menu (`Role::Menu`) for a screen reader.
- A file's menu keeps Paste (it goes beside the file, as the key's does); only New File and New Folder go. Paste is left out with nothing cut or copied, while a cut's paste waits, and when a cut entry would land where it is. Telling that needed `ProjectPanel::can_paste` in `project_panel_clipboard.rs` (its fields are private), added to the scope by respec.
- A right click on the empty space selects nothing, so its New File, New Folder and Paste act on the root.
- While the menu is open the panel's key context is `project_panel_menu`, which no binding names: Escape, Up, Down and Enter reach the menu's key listener (with vim on, Escape would otherwise be `dock.focus_editor`), and the tree's other keys wait. It closes on Escape, a press outside it, focus leaving the panel, any panel command (Refresh included), the tree being opened again, or its row going. `project_panel_nav.rs` needed no change.
- A right click while a name field is open is taken and dropped (the row's handler stops it before the panel's focus-on-press), so the field keeps focus and its text.
- The audit scenes are declared in `examples/screenshots/audit_scenes.rs` (there is no `audit/mod.rs`); the scope swapped it in by respec. The scene dispatches a real right press on the `src` row, then Down three times; its probes check the menu's ground and the highlighted Rename row, dark and light. On this Mac, 22 other surfaces' probes fail in the same audit run, none of them near the tree.
- `src/project_panel_menu.rs` joins the literal lint's `migrated` list. The carried finding T1561 F2 (References' dock title) was fixed on main by T1729: `dock.rs` titles it `workbench-dock-references-title`, and the allow entry's reason says so.
