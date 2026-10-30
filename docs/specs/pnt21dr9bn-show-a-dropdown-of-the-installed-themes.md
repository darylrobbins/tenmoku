---
task_uid: "01m62t30b11mn7mgpnt21dr9bn"
task: "T1233"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test settings_model_dynamic_choices::theme_keys_are_choices: SettingsModel entries for theme.dark, theme.light and theme.name have kind SettingKind::Choice(ChoiceSource::Themes(..)) while theme.mode stays SettingKind::Enum; every other Text entry is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test settings_editor::choices_tests::theme_dropdown_lists_installed_and_bundled: theme.name's dropdown (setting-select-theme.name) lists every fe_config::themes::bundled_themes() title; theme.dark lists only Dark-appearance themes and theme.light only Light, each from kiln::extensions::themes plus the bundled ones, in the order theme_picker.rs shows them"
    required: true
    manual: false
  - id: "C3"
    text: "settings_editor::choices_tests::picking_writes_the_id: picking an installed 'acme.solarized/solarized-dark' writes theme.dark = that id to the selected scope's file (comments kept); picking a bundled theme for theme.name writes its title"
    required: true
    manual: false
  - id: "C4"
    text: "settings_editor::choices_tests::unknown_value_still_shows: a file value not in the list (theme removed) shows as the dropdown's label with an 'not installed' hint and is not rewritten until another is picked; a Not set default shows 'Default (built-in)' as the first option that resets the key"
    required: true
    manual: false
  - id: "C5"
    text: "settings_editor::choices_tests::workspace_scope_is_off: theme.* rows stay restricted under the Workspace scope (existing behaviour, PROJECT_THEME)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo test -p fe-config and cargo test -p fe-workbench settings_editor pass; schema golden crates/fe-config/settings.schema.json is unchanged"
    required: true
    manual: false
---

# Show a dropdown of the installed themes for theme.dark, theme.light and theme.name in the Settings tab, from a list the app supplies at runtime

## Problem
`theme.mode` is a dropdown because its schema is an enum (SettingKind::Enum, settings_model.rs `kind`/`variants`, rendered by settings_editor_controls.rs `dropdown`). `theme.dark`, `theme.light` and `theme.name` are free `Option<String>` (settings.rs ThemeSettings), so they are schema strings and become plain text boxes (SettingKind::Text). Their valid values are only known at runtime: bundled themes (fe_config::themes::bundled_themes) and installed extension themes (kiln::extensions::themes(cx)), which theme_picker.rs already lists.
## Change
Add a runtime-choice kind: `SettingKind::Choice(ChoiceSource)` in settings_model.rs where `ChoiceSource` names a list the host resolves (`Themes { appearance: Option<Appearance> }`; later cards add models and fonts). SettingsModel assigns it by key path (an explicit table in settings_model.rs, not a schema keyword, so settings.schema.json and its golden stay unchanged). In the workbench, new settings_editor_choices.rs resolves a ChoiceSource to `Vec<EnumVariant>` (value + description = title/appearance) and settings_editor_controls.rs `control` renders it through the existing `dropdown` (generalise its `variants` argument; keep the select/highlight/Enter/Escape behaviour). Reuse the row-building and write path from theme_picker.rs (title vs id). Register the new test module with a `#[cfg(test)] #[path]` mod in settings_editor.rs, not fe-workbench/src/lib.rs (T1062/T1063 edit lib.rs).
Keep tests small: use the bundled themes and one fake installed theme through the existing extensions test hooks.

## Implementation notes
- `ChoiceSource::Themes { appearance }` is a struct variant, as the Change section names it; C1's `Themes(..)` reads as "any fields".
- Out-of-scope touches: `theme_picker.rs` gains `pub(crate) fn rows(&[ThemeInfo])` (the picker's row building, now shared) and makes `Row`/`Source` crate-visible; `crates/fe-config/tests/settings_model.rs` `ranges_come_from_schemars_attributes` asserted `theme.name` was `Text`, now `Choice`.
- Options: a slot (`theme.dark`/`theme.light`) lists the bundled themes of its appearance by `builtin/<id>` and the installed ones by ID, each described by its title; `theme.name` lists the bundled ones by title, described by appearance. A choice unset by default lists Default (built-in) first (`setting-option-default-<key>`), which resets the key. A value not in the list shows with a `not installed` hint (`setting-hint-<key>`).
- The write-path tests run again in their own process with `FASTEDIT_CONFIG_DIR` set (theme.* can't be written under the Workspace scope), as `settings_editor_controls_tests::user_scope_writes_user_file` does.
