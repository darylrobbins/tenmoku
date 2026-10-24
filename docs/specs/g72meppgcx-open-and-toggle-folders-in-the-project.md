---
task_uid: "01m5qphrj1dz8080g72meppgcx"
task: "T318"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/tree_vim_nav_keys.rs: with vim on in context project_panel, o resolves to project_panel.toggle and enter stays project_panel.open (day_one_bindings::resolves asserts it; both toggle a folder and open a file), shift-e to project_panel.expand_all, shift-w to project_panel.collapse_all, shift-h to project_panel.toggle_ignored and g r to project_panel.refresh, on macOS and Linux; with vim off none of the new keys resolves and enter still opens"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_nav_tests::toggle: o and enter on a collapsed folder expand it without moving the selection, on an expanded folder collapse it, and on a file open it in the editor (enter also in vim off); l and h keep T523's meaning"
    required: true
    manual: false
  - id: "C3"
    text: "project_panel_nav_tests::all: expand_all lists every folder under the root (bounded: stops at the existing listing cap and says so in the status text), collapse_all leaves only the root's children, selection ends on the nearest surviving row"
    required: true
    manual: false
  - id: "C4"
    text: "project_panel_nav_tests::ignored_and_refresh: toggle_ignored flips lists_ignored() and keeps the selected row if it still exists; refresh re-lists from disk so a file created outside the app appears with no watcher event"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench and cargo clippy -p fe-keymap -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited"
    required: true
    manual: false
---

# Open and toggle folders in the project tree with o and Enter, expand and collapse every folder, toggle ignored files and refresh, as nvim-tree does, in vim mode (Mac T1126)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The project tree has j k h l gg G a % r R d D Y gy (T523, keymaps/fastedit.toml:830-900) but cannot toggle a folder with o or Enter, expand or collapse everything, toggle hidden files, or refresh.

## Current state

- fastedit.toml:794-825 binds up/down/left/right/enter/f2/backspace; :834-900 the vim layer. `o` has no binding; `enter` is project_panel.open, which for a folder is not a toggle (verify in project_panel.rs before changing).
- Commands live in project_panel.rs (select_prev/next, collapse_or_parent, expand_or_first_child, open, select_first/last) and project_panel_ops.rs (T511). No expand_all, collapse_all, toggle_ignored or refresh command. `ProjectPanel::set_show_ignored`/`lists_ignored` (project_panel.rs:266-290) exist and re-open the tree.
- `R` is already rename (T523, netrw alias). nvim-tree makes R refresh; we keep R as rename so nothing shipped regresses, and bind refresh to `g r` (neo-tree/nvim-tree `R` conflict is the decision recorded here).

## Change

Copy nvim-tree's defaults (`o`/`<CR>` open or toggle, `E` expand_all, `W` collapse_all, `H` toggle dotfiles/ignored) with `g r` for refresh.
- New project_panel_nav.rs (declared from project_panel.rs): commands project_panel.toggle, expand_all, collapse_all, toggle_ignored, refresh, as impl methods on ProjectPanel. Enter/o on a folder toggles it; on a file opens it (existing open path).
- fastedit.toml: bindings in a new commented block `# Project tree: nvim-tree's o / E / W / H / refresh` directly after the existing `project_panel && vim` block. Per T523, none take a key from an editor context (they are all in project_panel).
- Register the commands the way project_panel.rs registers its others (check command_catalog_tests for the required metadata).

## Tests

New files only: fe-keymap tests/tree_vim_nav_keys.rs (bindings resolve; none in an editor frame) and fe-workbench project_panel_nav_tests.rs (declared with #[cfg(test)] #[path] from project_panel.rs, as the other panel tests are), using simulate_keystrokes against a tempdir tree as project_panel_ops_tests.rs does.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- **C1 respecced (rev 2).** As first written, C1 had vim-on `enter` resolve to `project_panel.toggle`, but `fe-keymap` `day_one_bindings::resolves` asserts that `enter` in `project_panel && vim` runs `project_panel.open`, and C5 forbids editing it. `project_panel.open` already toggles a folder and opens a file (`ProjectPanel::open_selected`, a click), so `enter` keeps it and `o` runs the new `project_panel.toggle`, which does the same. `day_one_bindings::vim_safe` also requires every `project_panel.open` binding to be listed in its card, which is another reason `o` gets its own command.
- **The listing cap.** The tree had no cap on expanding folders, so expand all brings its own: `project_panel::nav::EXPAND_ALL_LIMIT` = 300 folders, the same as nvim-tree's `actions.expand_all.max_folder_discovery`. Expansion goes breadth-first, one level per landed listing (each listing still runs off the main thread, through `reconcile`). When it stops at the cap, the panel shows a muted status line under its rows ("Expanded the first 300 folders: Expand All stops there"). `collapse_all`, a new expand all or a reopened tree clears the line.
- **Refresh** runs `apply_fs_changes(&[FsChange::Rescan])`, which is what the watcher sends when events were lost. The tree lists the root and every loaded folder again, and node ids, expanded folders and the selection all stay. The workbench's followers get the rescan too: disk sync checks the open files, and LSP logs it and sends nothing. As with the watcher's own rescans, the tree's `apply` does its listing on the main thread.
- **Toggle ignored** reopens the tree (`set_show_ignored`) and then restores it from paths: every folder that was expanded and is still listed is expanded again (an `Expansion` limited to those paths), and the selected row is revealed. A row that's no longer listed leaves nothing selected.
- **Collapse all** collapses every folder in `expanded`, so nested folders reopen collapsed too. A hidden selection moves to the depth-0 row it was under.
- **Review round 1 (a55rqc).**
  - F1: an expansion now ends only once no listing of the root or of a folder it took is running or owed. Before, a listing that `E` retried could land after the expansion had ended, and the folders it found stayed collapsed.
  - F2: `H` works in any state, the tree still opening included. What's still to restore (the folders that were expanded or waiting to expand, the ones the last restore hadn't reached, and the selection or its pending reveal) carries over to the next tree. `on_run_command` no longer cancels a pending reveal for `toggle_ignored`.
  - F3: `g r` on a root that failed to open opens it again (`ProjectPanel::reopen`, split out of `set_show_ignored`).
- Tests: `fe-keymap` `tree_vim_nav_keys::{macos,linux}`; `fe-workbench` `project_panel::nav_tests::{toggle,all,expand_all_stops_at_its_limit,expand_all_waits_for_the_listings_it_starts,ignored_and_refresh,toggle_ignored_again_while_the_tree_opens,refresh_opens_a_root_that_failed_to_open}`.
