---
task_uid: "01m4tqmay59rz7hbvv4b157mtk"
task: "T657"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-keymap tests/menu_terminal.rs: on the macOS and Linux tables, View has Toggle Terminal running terminal.toggle directly after Toggle Dock"
    required: true
    manual: false
  - id: "C2"
    text: "menu_terminal.rs: with the default keymap, Toggle Terminal's shortcut is ctrl-backtick"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-keymap and cargo clippy -p fe-keymap --all-targets -D warnings pass; tests/menu.rs unchanged"
    required: true
    manual: false
---

# Add Toggle Terminal (terminal.toggle) to the View menu, after Toggle Dock

## Problem

The user (2026-10-13): "Let's also add toggle terminal to the view menu."

The View menu lists Project, Search, Outline, Toggle Dock, Toggle Breadcrumbs, Status Bar and Soft Wrap
(`crates/fe-keymap/menus/fastedit.toml`, resolved per platform to `default-macos.toml` and
`default-linux.toml`, which fe-keymap's tests check match it exactly). The terminal opens only by
`terminal.toggle` (ctrl-backtick, context `workbench`) and the Terminal rail button (T601); no menu item runs it.

## Change

Add a `Toggle Terminal` item running `terminal.toggle` to the View menu, directly after `Toggle Dock`, in all
three files: `fastedit.toml` and its two resolved tables `default-macos.toml` and `default-linux.toml`. No
code change: the menu shows the item's shortcut from the active keymap, as for every item.

## Tests

New file `crates/fe-keymap/tests/menu_terminal.rs` (don't edit `tests/menu.rs`), following `tests/menu.rs`'s
helpers (copy the few it needs):

- On macOS and Linux tables, View contains `Toggle Terminal` → `terminal.toggle`, directly after
  `Toggle Dock`.
- With the default keymap its shortcut is ctrl-backtick.

## Out of scope

The rail button (T601, landed), dock positions (T603).
