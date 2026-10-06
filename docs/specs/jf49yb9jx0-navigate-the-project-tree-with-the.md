---
task_uid: "01m46tzwdnzedhd9jf49yb9jx0"
task: "T80"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::tree_keys::arrows_move_the_selection: with the panel focused, down selects the first row and then the next, up goes back, both clamp at the ends, and the selected row is drawn with TreeRow selected set (test counter of the selected row index in the last frame)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::tree_keys::left_and_right_follow_the_tree: right expands a collapsed directory (one listing, off the main thread) and then selects its first child; left on that child selects the directory and left again collapses it with no listing; left at the top level and right on a file change nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::tree_keys::enter_opens_or_toggles: enter on a file opens it in the active pane with its editor focused; enter on a directory toggles it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::tree_keys::the_selection_stays_in_view: in an expanded directory of 1,000 files, pressing down past the last visible row keeps the selected row inside the laid-out visible range"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::tree_keys::reveal_expands_only_unloaded_ancestors: opening a file 3 directories deep in a collapsed tree raises stats().listings by exactly 3, then selects its row inside the visible range without moving focus; an ignored file or one outside the root expands nothing"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::tree_keys::removing_the_selected_row_moves_the_selection: deleting the selected file through apply_fs_changes selects the row now at the same index, clamped to the last row"
    required: true
    manual: false
  - id: "C7"
    text: "project_panel.select_prev/select_next/collapse_or_parent/expand_or_first_child/open and project_panel.reveal_active are registered through the ACTION_TASK command registry; rg 'KeyBinding::new|actions!' finds no match in the files this card changes"
    required: true
    manual: false
---

# Navigate the project tree with the keyboard and reveal the active file

# Navigate the project tree with the keyboard and reveal the active file

Milestone M3 "Browse", third batch. It builds on:

- T63's `ProjectPanel`: `FileTree` rows in a `uniform_list`, with listings run on `cx.background_spawn`
  and filled on the main thread
- T63's fe-ui `TreeRow`, which has hover and selected tokens in `UiTheme`
- the action-model task's command registry

## Problem

The tree only responds to the mouse. Nothing shows which row you're on, and nothing finds the open
file in a large tree.

## Change

`crates/fe-workbench/src/project_panel.rs`:

- **A selected row.** The panel stores the selection as a `NodeId`, so it survives rows above it
  changing. Clicking a row selects it, as well as toggling or opening it as T63 does.
  - The selected row draws with `TreeRow`'s selected style. That's a `selected: bool` on `TreeRow` in
    `crates/fe-ui/src/tree_row.rs`, if T63 didn't add one.
  - Moving the selection keeps it scrolled into view (`uniform_list` scroll handle).
- **Keys**, when the panel has focus:
  - Down and Up move the selection by one row, clamped at the ends. With no selection, they select the
    first row.
  - Left on an expanded directory collapses it. Left on a file or collapsed directory selects its
    parent; at the top level it does nothing.
  - Right on a collapsed directory expands it, through the same background listing a click uses. Right
    on an expanded directory selects its first child. Right on a file does nothing.
  - Enter on a file opens it in the active pane and focuses its editor. Enter on a directory toggles
    it.
- **Reveal the active file.** When the active tab changes, and on the reveal command, the panel:
  - expands each collapsed ancestor of the file from the root down. Unloaded ancestors are listed
    off the main thread, one at a time, each only once.
  - then selects the file's row and scrolls it into view.

  It doesn't move keyboard focus. A file outside the root, or one the tree hides because it's
  ignored, reveals nothing and expands nothing. A newer reveal cancels a pending one. The
  active-tab-changed signal is a small hunk in `workbench.rs`, reusing the active-editor hook from
  the earlier cards if it exists.
- **Selection after file system changes.** When the selected node is removed by
  `apply_fs_changes`, the selection moves to the row now at the same index, clamped to the last row.
- `crates/fe-workbench/src/lib.rs`: one line in `init` to register commands.

### Commands

Register through the action-model task's registry. Don't add `KeyBinding::new` or `actions!`.

| ID | Default binding | Context |
|---|---|---|
| `project_panel.select_prev` / `project_panel.select_next` | `up` / `down` | `ProjectPanel` |
| `project_panel.collapse_or_parent` | `left` | `ProjectPanel` |
| `project_panel.expand_or_first_child` | `right` | `ProjectPanel` |
| `project_panel.open` | `enter` | `ProjectPanel` |
| `project_panel.reveal_active` | none (palette only, for now) | `Workbench` |

`dock.show_project` (from the search card) focuses the panel.

### Tests

New file `crates/fe-workbench/src/tests/tree_keys.rs`, declared in `tests/mod.rs`. It uses headless
GPUI, `fe_project::fixture` and a `tempfile` dir. It reads `FileTree::stats()` to count listings.

## Out of scope

- Type-to-select.
- Multi-select.
- Rename, create and delete.
- Home/End and Page Up/Down.
- A setting to turn auto-reveal off.

## Decided (lead defaults; the user may revisit)

- **Auto-reveal is on**: activating a tab reveals its file (the default in VS Code and Zed), without
  moving focus.
- Enter on a file opens it and focuses the editor. (On macOS VS Code uses Enter for rename; we have
  no rename yet.)
- The selection is kept by node, not by row index.
