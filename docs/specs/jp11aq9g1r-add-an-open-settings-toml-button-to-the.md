---
task_uid: "01m62t2dqb9awebnjp11aq9g1r"
task: "T1231"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test settings_editor::open_file_tests::button_opens_user_file: with Scope::User, clicking setting-open-file opens \u003cconfig dir\u003e/settings.toml in the active pane (created from the template if missing, as workbench.open_settings does) and the Settings tab stays open"
    required: true
    manual: false
  - id: "C2"
    text: "settings_editor::open_file_tests::button_follows_scope: with Scope::Workspace it opens \u003croot\u003e/.fastedit/settings.toml; with no folder open the Workspace scope and the label for it are absent as before"
    required: true
    manual: false
  - id: "C3"
    text: "settings_editor::open_file_tests::label_names_the_scope: label reads 'Open User settings.toml' / 'Open Workspace settings.toml'; the button shows a tooltip with the file path"
    required: true
    manual: false
  - id: "C4"
    text: "settings_editor::open_file_tests::workspace_file_missing_is_created_or_toasts: opening a missing workspace file either creates it from the template (preferred, same as the user's) or shows the existing 'Can't open settings' toast; no panic"
    required: true
    manual: false
  - id: "C5"
    text: "existing settings_editor_controls_tests edit_in_settings_toml tests pass unchanged (the per-table link stays)"
    required: true
    manual: false
---

# Add an Open settings.toml button to the Settings tab header that opens the selected scope's raw file

## What exists
Command `workbench.open_settings` ("Open Settings (JSON)", workbench_commands.rs, OpenOp::Settings in open.rs) opens only the USER file, and the Settings tab has `edit_in_file` (settings_editor_controls.rs) behind a per-row 'Edit in settings.toml' link, but only for SettingKind::Other rows (tables). Nothing on the page opens the file as a whole.
## Change
In settings_editor.rs `render`, put an 'Open settings.toml' button (setting-open-file) at the right end of the `scope_bar` row, calling `edit_in_file(&[], window, cx)` (an empty key path opens the file with no caret target; confirm `table_row` returns None for it and make it so if not). Make `edit_in_file` create the workspace file from the template too if the workspace file is missing (today `create` is true only for User). Also add a menu/palette-visible command only if one is missing: `workbench.open_settings` already exists, so do NOT add a new command. Wording follows Zed's 'Edit in settings.json' button.
