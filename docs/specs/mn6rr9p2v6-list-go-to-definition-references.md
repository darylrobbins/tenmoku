---
task_uid: "01m4gdrssrw338znmn6rr9p2v6"
task: "T279"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap test menu_go_nav: the default Go menu's item IDs (- for separators) are exactly nav.back, nav.forward, -, quick_open.toggle, nav.workspace_symbols, -, nav.goto_definition, nav.find_references, -, then the existing workbench.next_tab … project_panel.reveal_active items in their current order"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test menu_go_nav: with the default keymap, the shortcuts shown are ctrl-- for Back, ctrl-shift-- for Forward, cmd-t for Go to Symbol in Workspace…, f12 for Go to Definition and shift-f12 for Find All References"
    required: true
    manual: false
  - id: "C3"
    text: "Existing tests pass unchanged: crates/fastedit/tests/menus.rs default_menus_know_every_command and crates/fe-keymap/tests/menu.rs default_menus_parse"
    required: true
    manual: false
---

# List go-to-definition, references, workspace symbols, back and forward in the Go menu

# List go-to-definition, references, workspace symbols, back and forward in the Go menu

## Problem

After testing LSP go-to-definition (T144), the user asked: "Shouldn't we have this in the menubar too
though?"

The navigation commands are on main and reachable only by key, cmd-click and the pane toolbar's arrows.
None of them is in the menu bar. The Go menu in `crates/fe-keymap/menus/default-macos.toml` has Go to
File…, Next/Previous Tab, the four Focus … Pane items and Reveal Active File in Project.

These commands are all in `fe_workbench::command_catalog()` today, from T107/T112:

| command | catalog title | key (`crates/fe-keymap/keymaps/default-macos.toml`) | registered in |
|---|---|---|---|
| `nav.back` | Go Back | ctrl-- | `crates/fe-workbench/src/nav.rs` `registry()` |
| `nav.forward` | Go Forward | ctrl-shift-- | same |
| `nav.workspace_symbols` | Go to Symbol in Workspace… | cmd-t | same |
| `nav.goto_definition` | Go to Definition | f12 (and cmd-click) | same |
| `nav.find_references` | Find All References | shift-f12 | `crates/fe-workbench/src/references_panel.rs` `registry()` |

T144 adds no command and changes no binding. It changes only which provider answers these commands. So
this card works on main as it is and doesn't wait for T144.

There is no document-symbol command. The outline panel (`outline.show`) is already in the View menu, so
no item is added for it.

## Change

Edit only the `Go` menu in `crates/fe-keymap/menus/default-macos.toml`. Its items become, in this order
(`-` is a separator):

1. `Back`: `nav.back`
2. `Forward`: `nav.forward`
3. `-`
4. `Go to File…`: `quick_open.toggle` (existing)
5. `Go to Symbol in Workspace…`: `nav.workspace_symbols`
6. `-`
7. `Go to Definition`: `nav.goto_definition`
8. `Find All References`: `nav.find_references`
9. `-`
10. The existing Next Tab … Reveal Active File in Project items, unchanged and in their current order

The titles follow the Finder and VS Code Go menus. Back and Forward drop "Go" because they sit in a menu
already named Go. Shortcuts come from the keymap as usual, so no key goes in the table. The menu shows
F12 for Go to Definition: cmd-click is a mouse binding, and `CommandItem::shortcut` shows only a single
keystroke, taking the later f12 entry first.

**Enabled state.** On gpui-pre 0.3.7 every menu item is enabled. This is the known limit in
`crates/fe-gpui/src/menu.rs`'s module doc and in T117, which the user accepted. An item whose command
doesn't apply where focus is runs nothing, as its key wouldn't. These items get the same behaviour. The
card doesn't try to disable them.

**Tests.** Add `crates/fe-keymap/tests/menu_go_nav.rs`, with its own small helpers in the style of
`tests/menu.rs`. It does two things:

- It pins the Go menu's item IDs, with `-` for separators, exactly as in the list above.
- It checks each new item's shortcut against the default keymap.

The existing tests need no change:

- `crates/fastedit/tests/menus.rs` checks that every menu command is in the catalog.
- `crates/fe-keymap/tests/menu.rs` `default_menus_parse` rejects duplicate IDs and a leading or trailing
  separator.
