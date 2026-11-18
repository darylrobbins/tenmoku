---
task_uid: "01m7p4mj4989tdrvxfzwr7e67e"
task: "T681"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "A new command project_panel.new_menu (titled in commands.ftl, listed in the palette) opens the New menu under the +, focused; Up/Down/Enter choose an entry; with no folder it opens what T1937 shows there"
    required: true
    manual: false
  - id: "C2"
    text: "It has a default key on Linux and macOS that fires from the focused tree (no secondary- clash; DEFAULT_LINUX and DEFAULT_MACOS both list it); regression test tests::project_new_menu::keys::new_menu_key_opens_the_menu_from_the_tree sends the Linux keystroke and fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "The menu's File and Folder entries show a key outside vim mode (project_panel.new_file / new_folder get non-vim bindings)"
    required: true
    manual: false
  - id: "C4"
    text: "Checked in the real Linux app (Xvfb + xdotool, run-fastedit-linux): focus the tree, press the key, arrow to Folder, Enter, type a name; screenshots in the finish evidence; also covered by the headless test"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench project_new default_keymaps passes; cargo clippy -p fe-workbench -p kiln-shell --all-targets is clean"
    required: true
    manual: false
---

# Project tree's New menu has no keyboard route on Linux: no key or command opens the + menu, and new file/folder have keys only in vim (Mac T1938)

User report (Linux, PRIORITY): keyboard shortcuts don't reach the Project header + for new projects. Tab doesn't walk the dock header (IconButton is a tab stop, kiln-ui controls.rs:225, but nothing binds Tab to window.focus_next), no command opens the New menu, and project_panel.new_file/new_folder have no non-vim keys (fastedit.toml:1346). Add a palette command that shows the Project dock and opens the New menu under its + (anchor from the last-drawn button bounds), bound on every platform (JetBrains' alt-insert elsewhere, cmd-n in the tree on macOS, or the worker's choice recorded in the decision file).

## Helium filing note

- USER PRIORITY (a Linux user couldn't start a project). Lent from the Mac as y140.
- Test layout differs from the Mac card on purpose: the new tests go in a submodule of the existing
  `src/tests/project_new_menu.rs` (`mod keys;` there, file `src/tests/project_new_menu/keys.rs`), not a
  new `src/tests/<x>.rs` from `tests/mod.rs`. On helium a new module from `tests/mod.rs` gives "0 tests" at
  base and fails a bugfix finish as needs_human:infra. `tests/mod.rs` is left out of scope for that reason.
  Regression test and criteria use the path `tests::project_new_menu::keys::...` (bare, no crate prefix),
  in the card and in the finish criteria file.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; put the screenshots in the finish
  evidence. Kill only processes you started, by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
