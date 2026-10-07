---
task_uid: "01m492vy8wxf4pn13fdxptwhy7"
task: "T124"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test tests/template.rs::template_is_all_defaults: USER_SETTINGS_TEMPLATE loaded as the user layer gives Settings::default() and no diagnostics; every EditorSettings key (font_family, font_size, line_height, tab_size, soft_wrap) appears in it commented out with its default value"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::open_settings::creates_and_opens_the_file: with the config directory overridden to an empty tempfile dir, cmd-comma creates \u003cdir\u003e/settings.toml with exactly USER_SETTINGS_TEMPLATE and opens it as the active pane's active tab; a second cmd-comma activates that tab without rewriting the file"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::open_settings::an_existing_file_is_never_overwritten: with settings.toml already holding font_size = 16, cmd-comma opens it unchanged; with no config directory (the override set to None) nothing opens, no tab changes and an error is logged"
    required: true
    manual: false
  - id: "C4"
    text: "workbench.open_settings (cmd-comma, workbench) is registered through the command registry and known to fe_workbench::command_catalog(); the app menu lists Settings… after its first separator and fastedit's tests/menus.rs::default_menus_know_every_command still passes; command_tests and keymap_tests pass with only their pinned counts updated; rg 'KeyBinding::new|actions!' finds no match in the files this card adds"
    required: true
    manual: false
---

# Open the user's settings.toml in a tab from Settings… (cmd-comma)

# Open the user's settings.toml in a tab from Settings… (cmd-comma)

A follow-up to the menu-bar card (T117), whose app menu has no Settings… because no command existed. It
builds on:

- fe-config: `user_settings_path()` (`<config_dir>/settings.toml`), `Settings`, `EditorSettings`,
  `SettingsStore` and the config watcher, which already reloads that file when it's saved
- T63's `Pane::open` and T79's active pane
- the open-folder card's (T123) command module, `crates/fe-workbench/src/open.rs`
- the menu table (`crates/fe-keymap/menus/default-macos.toml`)

## Problem

Settings live in `~/.config/fastedit/settings.toml`, which nothing in the app opens. A new user has no
file at all, and no way to learn the keys.

## Change

### The template

New data file `crates/fe-config/settings-template.toml`. `crates/fe-config/src/lib.rs` exports it as
`pub const USER_SETTINGS_TEMPLATE: &str = include_str!(…)`.

- A header comment says what the file is, that it reloads on save, and that a project's
  `.fastedit/settings.toml` overrides it.
- Then each `[editor]` key (`font_family`, `font_size`, `line_height`, `tab_size`, `soft_wrap`), each
  **commented out**, with its default value and a one-line description.
- Loaded as it is, it gives exactly `Settings::default()` and no diagnostics.

### The command

`workbench.open_settings`, titled "Settings…", in the `workbench` context, bound to `cmd-,`. It's
registered in `crates/fe-workbench/src/open.rs` beside the open-folder commands, in the same registry
pattern. Don't add `KeyBinding::new` or `actions!`.

- **Opening the file.** It resolves `fe_config::user_settings_path()`.
  - If the file is missing, it creates the parent directory and writes `USER_SETTINGS_TEMPLATE` with
    `create_new(true)`, so it never overwrites a file made in the meantime. This is done off the main
    thread, on `cx.background_spawn`.
  - It then opens the file in the active pane through `Pane::open`. If it's already open, that tab
    activates.
- **Failures.** With no config directory (`None`), or a failed write, it logs the error and opens
  nothing.
- **Reload.** Saving the tab writes the file, and the existing watcher reloads it. This card adds no
  reload wiring.
  - If the file didn't exist at launch, check that the watcher picks up the newly created file (its
    paths include the user settings path either way). If it doesn't, make it, within
    `crates/fastedit/src/main.rs`'s `watch_config`, and say so.

### Menus and keys

- `crates/fe-keymap/menus/default-macos.toml`: the app menu gets Settings… (`workbench.open_settings`)
  after its first separator, where macOS apps put it.
- `crates/fe-keymap/keymaps/default-macos.toml`: `cmd-,` binds to `workbench.open_settings` in
  `workbench`. Update the pinned counts in `crates/fe-gpui/src/command_tests.rs` and
  `crates/fe-gpui/src/keymap_tests.rs`.
- `crates/fe-workbench/src/workbench.rs` changes only to do the op.

### Tests

- New file `crates/fe-config/tests/template.rs` (an integration test, so `src/tests/mod.rs` isn't
  touched).
- New file `crates/fe-workbench/src/tests/open_settings.rs`, declared in `tests/mod.rs`. It uses headless
  GPUI. The config directory comes from a `tempfile` dir through a test-only override of the path,
  because the test mustn't touch the real `~/.config`.

## Out of scope

- A settings UI.
- Opening `keymap.toml` (a natural sibling, `workbench.open_keymap`, could follow the same way).
- Opening the project's settings file.
- Schema-aware completion or validation squiggles in the tab. The existing diagnostics are only logged.

## Decided (lead defaults; the user may revisit)

- Settings… opens the raw TOML in an editor tab, as Zed does, with no settings UI.
- A missing file is created from a fully commented template, so it changes no behaviour until edited.
- `cmd-,` is the key, as is standard on macOS.

## Implementation notes

- The watcher skips a missing directory, so a config directory created after launch would not be watched. `watch_config` in `crates/fastedit/src/main.rs` now creates the user config directory before watching.
- The test-only config directory override is `open::ConfigDirOverride` (a `#[cfg(test)]` global); `None` models no config directory.
