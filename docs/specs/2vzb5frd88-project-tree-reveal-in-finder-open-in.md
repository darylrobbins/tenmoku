---
task_uid: "01m78rqva3m4cafm2vzb5frd88"
task: "T335"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::project_os_commands::reveal_records_the_selected_path_through_the_seam, file_row_reveals_its_file, open_in_terminal_uses_the_folder_or_parent, find_in_folder_scopes_to_the_folder, menu_lists_the_three_items pass"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests, tests::find_keys and terminal_actions_tests pass; cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Project tree: Reveal in Finder, Open in Terminal and Find in Folder on the selected row (Mac T1751)

# Project tree: Reveal in Finder, Open in Terminal, Find in Folder on the selected row

VS Code's Explorer and IntelliJ's Project view both offer these on a row; the tree has only the active-file versions (`workbench.reveal_active_file_in_os`, path_commands.rs behind `RevealSeam`).

## Change (new commands in the `project_panel` context, new file `crates/fe-workbench/src/project_panel_os.rs`; do NOT edit path_commands.rs or external_drop.rs, held by T1563)
- `project_panel.reveal_in_os`: reveal the selected path through the same seam as the active-file command (`App::reveal_path`; "Reveal in Finder" on macOS, "Reveal in File Manager" elsewhere, title picked through the existing platform seam, no new `cfg(target_os)` outside kiln's platform dirs).
- `project_panel.open_in_terminal`: a new terminal in the selected folder (a file's parent), as the terminal dock already opens one in a cwd.
- `project_panel.find_in_folder`: opens Find in Files scoped to the selected folder (a file's parent).
- Added to the context menu from the menu card (items at the foot of the path group); no default keys; palette lists them while the tree has focus.
- Strings in project.ftl.

## Pins
tests::project_menu (from the menu card), project_panel_ops_tests, tests::find_keys, terminal_actions_tests, typed_command_ids_tests (its listed titles gain the new commands).
No golden moves.
Platform: linux-ok (the reveal call is behind the seam; a test records the path).
