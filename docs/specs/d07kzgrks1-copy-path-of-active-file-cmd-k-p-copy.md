---
task_uid: "01m5w61q2nhhwk7gd07kzgrks1"
task: "T359"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "path_commands_tests::copy_path: workbench.copy_active_file_path puts the active file editor's absolute path on the clipboard (native separators), workbench.copy_active_file_relative_path the path relative to the window folder with / separators (the absolute path if the file is outside it), and both leave the clipboard alone for an untitled buffer, a tab without a file, or no editor"
    required: true
    manual: false
  - id: "C2"
    text: "path_commands_tests::copy_follows_the_focused_pane: with two panes open on different files the command uses the active pane's active tab, and a file renamed in the project tree copies its new path"
    required: true
    manual: false
  - id: "C3"
    text: "path_commands_tests::reveal: workbench.reveal_active_file_in_os calls the reveal seam with the active file's absolute path (Finder via App::reveal_path on macOS, the file manager on Linux), and does nothing for an untitled buffer"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap active_file_path_key: secondary-k p resolves to the copy command and secondary-k r to the reveal command in the workbench context on both platforms; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass Known limit: the chords are asserted on the parsed bindings and catalog, not driven through GPUI key input (T370)."
    required: true
    manual: false
---

# Copy Path of Active File (cmd-k p), Copy Relative Path of Active File and Reveal Active File in OS (cmd-k r), as VS Code's workbench.action.files.copyPathOfActiveFile, copyRelativePathOfActiveFile and revealActiveFileInWindows (Mac T1188)

The project tree has Copy Path / Copy Relative Path (project_panel.copy_path, project_panel_ops.rs ~754) and project_panel.reveal_active (select in the tree), but nothing runs on the active editor tab. New module path_commands.rs with its own "#[kiln::command] impl Workbench" block (pattern: find_commands.rs), so workbench_commands.rs is not edited. Commands: workbench.copy_active_file_path ("Copy Path of Active File"), workbench.copy_active_file_relative_path ("Copy Relative Path of Active File"), workbench.reveal_active_file_in_os ("Reveal Active File in File Manager"; the title says Finder on macOS only if the catalog titles may be platform-specific, else keep the neutral one).

Relative path: against the window's folder root (the same root project_panel_ops uses), "/" separators as that code does; share its helper rather than duplicating. Active file: Workbench::active_editor(cx).model().path(), as reveal_active (workbench_chrome.rs ~311) does.

Reveal: gpui's App::reveal_path is a no-op under the test platform, so route it through a small seam in path_commands.rs (a pub(crate) fn pointer or Global set by tests) that records the path; production calls cx.reveal_path. No new process spawning.

Keys, VS Code's default macOS keymap: cmd-k p copy path, cmd-k r reveal (same on Linux via secondary). VS Code has no default key for the relative-path command; leave it palette-only. Verify "secondary-k p" and "secondary-k r" are free (they were at drafting time; "secondary-k v" markdown preview and "secondary-k z" zen exist). Menus untouched.

Lent from Mac T1188 (y73). Linux-OK: reveal goes through a test seam in path_commands.rs; keys use secondary-k.
