---
task_uid: "01m7p4mhk9waecn5kcevsf7fj8"
task: "T680"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "With no folder open, the Project panel's empty state shows an Open Folder… button (keyboard focusable, showing its key) that runs workbench.open_folder; a rendered test clicks it and sees the command dispatched"
    required: true
    manual: false
  - id: "C2"
    text: "With no folder open, clicking the + opens a menu (with Open Folder… at least) or the + is not drawn; it never silently does nothing (regression test tests::project_new_menu::no_folder::plus_with_no_folder_offers_open_folder fails at base)"
    required: true
    manual: false
  - id: "C3"
    text: "The + tooltip/accessible name no longer is 'New…' or any text an item of its menu uses (e.g. 'New File or Folder'); test asserts the dock's last_tooltip"
    required: true
    manual: false
  - id: "C4"
    text: "With a folder open, a rendered test moves the pointer from the + down onto the menu's first item and the menu stays open and highlights it; the existing project_new_menu tests pass"
    required: true
    manual: false
  - id: "C5"
    text: "Checked in the real Linux app (Xvfb + xdotool, run-fastedit-linux): no folder open, hover then click +, then the empty-state button; screenshots in the finish evidence; also covered by the headless tests above"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench project_new_menu passes; cargo clippy -p fe-workbench --all-targets is clean"
    required: true
    manual: false
---

# Project sidebar with no folder open: the + does nothing and its New… tooltip reads like a menu item, so a Linux user can't start a project (Mac T1937)

User report (Linux, PRIORITY): a user with "No folder open" was confused that they couldn't create a project. They clicked the Project header + to make one, nothing happened, and the "New…" box under it (the tooltip) vanished when they moved to it. open_new_menu returns with no tree (project_panel_new_menu.rs:104); the empty state is bare text (project_panel.rs:1741, message() at :1501). There is no "new project" command; today's flows are workbench.open_folder (File > Open Folder, start page "Open Folder"). Make the empty Project sidebar offer a clear start (Open Folder…, through workbench.open_folder) and make the + do something visible with no folder; rename the + tooltip so it doesn't read as a menu entry. The + is the PaneAction NEW_BUTTON (project_panel.rs:191-210); kiln draws its tooltip in kiln/crates/kiln-shell/src/dock.rs:1386.

## Helium filing note

- USER PRIORITY (a Linux user couldn't start a project). Lent from the Mac as y140.
- Test layout differs from the Mac card on purpose: the new tests go in a submodule of the existing
  `src/tests/project_new_menu.rs` (`mod no_folder;` there, file `src/tests/project_new_menu/no_folder.rs`), not a
  new `src/tests/<x>.rs` from `tests/mod.rs`. On helium a new module from `tests/mod.rs` gives "0 tests" at
  base and fails a bugfix finish as needs_human:infra. `tests/mod.rs` is left out of scope for that reason.
  Regression test and criteria use the path `tests::project_new_menu::no_folder::...` (bare, no crate prefix),
  in the card and in the finish criteria file.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; put the screenshots in the finish
  evidence. Kill only processes you started, by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
