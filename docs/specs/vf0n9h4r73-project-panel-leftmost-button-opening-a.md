---
task_uid: "01m79cxdrqftmydsvf0n9h4r73"
task: "T597"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests::project_new_menu::{plus_is_the_first_pane_action,menu_lists_file_folder_then_typed_files,detected_languages_come_first,choosing_python_creates_a_py_file_with_the_template,name_field_keeps_extension,target_is_the_selected_folder,escape_closes_without_creating} pass"
    required: true
    manual: false
  - id: "C2"
    text: "tests::project_panel, project_panel_ops_tests, a11y_tabs_tree, dock_titles_locale and the kiln-shell dock tests pass; the screenshots example builds on Linux (its goldens are Metal's: workbench.png and the audit scenes showing the project header are regenerated and reviewed on the Mac after the sync)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Project panel: leftmost + button opening a New menu (File, Folder, typed files with templates by detected project language) (Mac T1769)

# Project panel: a leftmost "+" button opening a New menu (File, Folder, typed files with templates)

Today the dock header (kiln-shell dock.rs PaneAction row, right-aligned, before ⋯ and hide) has Reveal and Collapse All (project_panel.rs `pane_actions`). New File / New Folder exist as `project_panel.new_file` / `project_panel.new_folder` (PanelOp, tree right-click menu T1750, project_panel_menu.rs) and open an inline name field (project_panel_ops.rs EditKind); there are NO file templates and no project-language detection (fe-syntax detect is path/extension-based).
## Change
- `pane_actions` returns `+` (Icon::Plus; add the icon to kiln-ui icons at 1.75 stroke if absent, NOTICE if from a set) FIRST, so it is the leftmost action. Clicking opens a dropdown under the button. PaneAction has only `run`; add an optional anchor (the button's bounds) passed to run, or a `menu: Option<..>` field, in kiln-shell dock.rs; gallery goldens of docks keep passing (no `+` outside the project panel).
- Menu (reusing project_panel_menu MenuPanel, target folder = selected folder / the selected file's parent / root, as New File does today): File | Folder | then typed files: Markdown (.md), plus up to 6 chosen by project markers at the root (Cargo.toml -> Rust file; package.json/tsconfig.json -> TypeScript, JavaScript; pyproject.toml/requirements.txt -> Python; go.mod -> Go; pom.xml/build.gradle -> Java), always HTML, CSS, JSON, Shell script as the fallback set. Detected languages come first.
- Choosing a typed entry opens the same inline name field, prefilled with a stem and the extension selected-stem-only (`Untitled.py`), and creates the file with a tiny template body (trivial: empty, or one line: shebang for sh, `<!doctype html>` skeleton for HTML). Template table is data in new project_new_templates.rs.
- Created files open as T1750's New File does (PanelEvent::Created).
## Scope
crates/fe-workbench/src/project_panel.rs, project_panel_ops.rs, project_panel_menu.rs, new project_panel_new_menu.rs and project_new_templates.rs, kiln/crates/kiln-shell/src/dock.rs, kiln-ui icon files if needed, l10n/en-US/project.ftl, new tests/project_new_menu.rs declared in tests/mod.rs (plus_is_the_first_pane_action, menu_lists_file_folder_then_typed_files, detected_languages_come_first, choosing_python_creates_a_py_file_with_the_template, name_field_keeps_extension, target_is_the_selected_folder, escape_closes_without_creating).
## Pins/goldens
tests::project_panel, project_panel_ops_tests, a11y_tabs_tree, dock title tests (dock_titles_locale). Goldens that move: any screenshot showing the project panel header (workbench.png, setup/tree scenes; regenerate and check with `rg -l project-reveal` in tests/audit); kiln-shell gallery goldens only if the dock gallery uses a project-like action set.
Platform: linux-ok.

## Implementation notes

- **Anchor.** `PaneAction` gained `anchored: Option<AnchoredHandler>`
  (kiln-shell dock.rs): an action that has one gets its button wrapped in a
  div tracking its bounds, and a click runs `anchored(bounds, …)` in place of
  `run`. Only the project panel's `+` (`project-new`) sets it; search's and
  the other actions pass `None` (search_panel.rs needed that field, hence the
  scope).
- **Menu.** `project_panel_new_menu.rs` keeps its own `NewMenu` state beside
  the right-click `TreeMenu`, drawn with fe-ui's `MenuPanel` under the `+`
  (top-left at the button's bottom-left, snapped into the window). Both menus
  share `MENU_KEY_CONTEXT`, `close_menu` (closes either) and the highlight
  stepping (`menu::next_highlight`). Escape gives focus back to what had it
  outside the panel; a second click on `+` closes it. The `+`'s plain `run`
  (no bounds to anchor to) starts New File, the menu's first entry.
- **Entries.** File | Folder | detected types (Cargo.toml → Rust;
  package.json/tsconfig.json → TypeScript, JavaScript; pyproject.toml /
  requirements.txt → Python; go.mod → Go; pom.xml / build.gradle(.kts) →
  Java; at most 6, each once, read from the root's listed entries, no file
  system access), then Markdown, then HTML, CSS, JSON, Shell Script, always.
- **Templates.** `project_new_templates.rs`: empty bodies except Go
  (`package main`), HTML (a `<!doctype html>` skeleton), JSON (`{}`) and
  Shell (`#!/bin/sh`). The field is prefilled `Untitled.<ext>` (the first of
  `Untitled-2.<ext>`… free among the folder's listed entries), stem
  selected. The body is written only if the name still ends in the type's
  extension. Writing goes through new `fe_project::fs_ops::create_file_with`,
  into the O_EXCL handle `create` opened, so it never lands in anything else
  at the path; a failed write removes the file it made (scope: fs_ops.rs).
- **Icon.** `Icon::Plus` is Lucide's `plus` at 1.75 stroke (in Lucide's
  Feather list, MIT); `kiln/THIRD_PARTY.md` lists it (scope: file_icons.rs,
  assets/icons/plus.svg, THIRD_PARTY.md).
- **T569 F6.** The dock's ⋯ menu, removed with no window to move focus in
  (`set_visible(false)`, as zen mode hides the sidebar, or `set_menu(vec![])`),
  now keeps what had focus before it; a GPUI `on_focus_lost` watch gives it
  focus back once the frame without the menu is drawn, and an `on_blur` watch
  drops it if the host moved focus on first. The sidebar shortcut already
  closed the menu first (fe-workbench `DockOp::Toggle`). Tests:
  `tests::sidebar_b3::{the_sidebar_shortcut_hiding_the_docks_menu_gives_focus_back,
  hiding_the_dock_or_clearing_its_menu_gives_focus_back}` (scope: sidebar_b3.rs,
  which also pins the header's buttons, now `+` first).
- **Goldens.** The screenshots example renders only on macOS (Metal);
  workbench.png and the audit scenes showing the project header move by the
  `+` and are to be regenerated and reviewed on the Mac after the sync (C2
  respecced to say so).
- **activity_rail.rs.** `only_the_shown_panel_is_highlighted` clicked the
  middle of the header row to check the title row ignores clicks; the `+`
  now sits there in a 240 px sidebar, so it clicks the title (24 px in)
  instead (scope respecced).
