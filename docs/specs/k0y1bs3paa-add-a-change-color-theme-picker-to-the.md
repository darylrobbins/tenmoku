---
task_uid: "01m57zd9thqyy87tk0y1bs3paa"
task: "T203"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench unit test theme_picker_tests::choosing_a_bundled_theme_applies_and_keeps_it: dispatching theme.select lists every bundled theme and the installed ones with the active one marked; confirming Dark Modern writes theme.name = \"Dark Modern\" to the user settings.toml (a temp file with other keys and comments, which stay) and the active theme's palette becomes Dark Modern's after the reload"
    required: true
    manual: false
  - id: "C2"
    text: "theme_picker_tests::choosing_an_installed_theme_sets_its_slot: confirming an installed dark theme writes theme.dark = its id and clears theme.name; the filter narrows rows by title; Escape closes the picker and changes nothing"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-core test write_setting_str::sets_and_inserts_strings: an existing string is replaced, a missing key is inserted under its table, a missing file starts from the template, comments are kept, and a quote in the value is escaped"
    required: true
    manual: false
  - id: "C4"
    text: "typed_command_ids_tests lists theme.select as \"Change Color Theme…\"; cargo nextest run -p fe-workbench -p kiln-core -p fe-config passes with no existing test edited beyond that row; cargo clippy -p fe-workbench -p kiln-core -p fe-config --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Add a Change Color Theme picker to the command palette that applies a bundled or installed theme and keeps it in settings.toml (Mac T998)

## Problem

The user asked (2026-10-18) for a command-palette theme picker beside the `[theme] name` setting (the card this depends
on). Today a theme is chosen by editing `settings.toml`. VS Code has "Preferences: Color Theme" (a picker over the
installed themes that applies the choice and keeps it).

## Change

`theme.select` ("Change Color Theme…", in the command palette) opens a picker over the bundled themes
(`fe_config::themes::bundled_themes()`) and the installed extension themes (`kiln::extensions::themes(cx)`), each row
showing its title and appearance (dark or light), the active one marked. Choosing a row applies it and writes it
to the user's `settings.toml`.

- `crates/fe-workbench/src/theme_picker.rs` (new): a `Picker` delegate modelled on `language_picker.rs`
  (`LanguagePicker`): rows, filter, current mark, `confirm`. A bundled theme sets `theme.name` to its title; an
  installed theme sets `theme.dark` or `theme.light` (its appearance's slot) to its id and clears `theme.name` for
  that slot. Confirming applies at once through the settings reload the write triggers, as every other setting does.
- Wiring, as `language.select` is: a `theme_picker` entity and accessor in `crates/fe-workbench/src/workbench.rs`, the
  `mod` line in `crates/fe-workbench/src/lib.rs`, the command in `crates/fe-workbench/src/workbench_commands.rs`, and
  its row in the `COMMANDS` list of `crates/fe-workbench/src/typed_command_ids_tests.rs` (the one existing test file
  edited).
- A string write: `kiln/crates/kiln-core/src/write.rs` gains `write_setting_str(path, key, value: &str, template)`
  beside `write_setting` (same atomic, comment-keeping rewrite; a missing key is inserted under its table), and
  `crates/fe-config/src/write.rs` + `lib.rs` export `write_setting_str` as `write_setting` does.
- Tests, new files: `crates/fe-workbench/src/theme_picker_tests.rs` (declared from `theme_picker.rs`),
  `kiln/crates/kiln-core/tests/write_setting_str.rs`.

## Implementation notes

- Clearing `theme.name` needs a removal, so kiln-core's `write.rs` has `write_settings(path, changes, template)`:
  each change sets a value (`SettingValue::Bool` or `Str`) or, for `None`, removes the key, all in one atomic
  write, so a pick is one reload. `write_setting` and `write_setting_str` wrap it; fe-config exports all three.
  Strings are basic strings, escaped by hand. A removed key takes its line (and its trailing comment) when
  that leaves the same settings as `toml_edit`'s removal does, else `toml_edit` removes it. A commented-out
  default is replaced only by a value of its type. Changes that leave the file as it was write nothing.
- The picker writes the pick and leaves applying it to the reload. A bundled pick sets `theme.name` and removes
  the slot's `theme.dark`/`theme.light`, which would win. An installed pick sets the slot and removes a
  `theme.name` of that appearance. In both cases the other slot keeps its theme: if the old name chose it,
  it's written as `builtin/<id>`. If the other appearance shows, whether `theme.mode` sets it or follows the
  system, `theme.mode` turns to the pick's (review 2ede6j F1). Without that, the pick wouldn't show.
- The installed rows are `kiln::extensions::themes` without `builtin/*`, the bundled ones under their own titles.
  The current mark follows the host's choice for the appearance showing, checked as kiln checks it: an ID not
  installed with that appearance, or no choice, marks Fastedit Dark/Light, which kiln falls back to (F2).
- The fe-workbench tests run kiln's config service with a test host standing in for `crates/fastedit/src/theme.rs`
  (it selects themes and mode from `[theme]`), since fe-workbench can't depend on fastedit.
