---
task_uid: "01m6hxegjeeammp2smd6dmf2tm"
task: "T1339"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "setup_wizard_model_tests::steps_in_order: steps() is Appearance, Font, Keyboard, Editing, Ready; Keyboard lists vim.preset and editor.line_numbers only while Vim is chosen; Appearance lists one theme list for dark or light mode and two for system"
    required: true
    manual: false
  - id: "C2"
    text: "setup_wizard_model_tests::starts_from_current_settings: from Settings with theme.name = \"Ember\", vim.enabled = true, editor.font_size = 15 and editor.auto_save = on_window_change, the choices show Tenmoku Ember in the dark slot, Vim, 15 and Custom"
    required: true
    manual: false
  - id: "C3"
    text: "setup_wizard_model_tests::unchanged_choices_write_nothing: choosing what each control already shows returns no writes, and a missing settings file stays missing"
    required: true
    manual: false
  - id: "C4"
    text: "setup_wizard_model_tests::changed_choices_write_only_their_keys: on a temp settings.toml with comments, choosing Light, Catppuccin Latte, Vim and font size 15 leaves the comments, sets exactly theme.mode = \"light\", theme.light = \"builtin/catppuccin-latte\", vim.enabled = true and editor.font_size = 15, and SettingsStore::load gives no diagnostics"
    required: true
    manual: false
  - id: "C5"
    text: "setup_wizard_model_tests::back_to_default_removes_the_key: font size 15 then 13 removes editor.font_size; a dark theme then Tenmoku Dark removes theme.dark; Default (\u003cfamily\u003e) removes editor.font_family"
    required: true
    manual: false
  - id: "C6"
    text: "setup_wizard_model_tests::theme_name_gives_way_to_the_slot: with theme.name = \"Ember\" set, choosing Dark 2026 writes theme.dark = \"builtin/vscode-2026-dark\" and removes theme.name, and loading the file warns about nothing"
    required: true
    manual: false
  - id: "C7"
    text: "setup_wizard_model_tests::every_control_is_labelled: every control on every step, in every choice state, has a non-empty title, description and option labels, and its key is in fe_config::settings_schema()"
    required: true
    manual: false
  - id: "C8"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean (the model's mod line carries cfg_attr(not(test), allow(dead_code)) until the view uses it)"
    required: true
    manual: false
---

# Model the first-run setup wizard headless: five steps (Appearance, Font, Keyboard, Editing, Ready), the curated choices on each with their settings keys, and writes that keep only what the user changed

## Problem

The user asked (2026-11-03) for "an on-boarding wizard for the first time a user starts the editor to let them
pick their preferences". This card is its headless model: what the wizard asks, in which order, and what each
answer writes to the user's `settings.toml`. No GPUI view here; the sheet is a later card.

## Change

New `crates/fe-workbench/src/setup_wizard_model.rs` (no GPUI types, no `cx`), declared from
`crates/fe-workbench/src/lib.rs` with `#[cfg_attr(not(test), allow(dead_code, reason = "the setup sheet uses it"))]`
on the `mod` line (its only caller is the view card; vet's lib build sees it dead otherwise). Tests in new
`crates/fe-workbench/src/setup_wizard_model_tests.rs`, declared `#[cfg(test)] mod setup_wizard_model_tests;` in
`lib.rs` beside `update_check_tests`.

### Steps, in order

1. **Appearance**: `theme.mode` as three choices, Dark / Light / Match System (`dark`, `light`, `system`;
   default `dark`). Then a theme for each appearance the mode shows (one list for Dark or Light, two for Match
   System), from `fe_config::themes::bundled_themes()` filtered by appearance. Written to the slot,
   `theme.dark` / `theme.light`, as the ID kiln selects it by (`kiln::extensions::builtin_theme_id(theme.id)`,
   `builtin/<id>`), as `settings_editor_choices.rs` documents for the slots. An unset slot is the bundled
   Tenmoku Dark / Tenmoku Light (kiln's base chrome with `fe_config::EditorPalette::fastedit_dark`/`_light`),
   so that tile shows as chosen when the slot is unset, and choosing it removes the key. If `theme.name` is set
   (a re-run from the command), it is what the slot of its appearance shows; setting that slot removes
   `theme.name` (as `theme_picker.rs:changes` does), so loading gives no "theme.name is ignored" warning.
2. **Font**: `editor.font_family` (unset = the first installed of `kiln::platform::fonts::MONO_FAMILIES`;
   the model takes the list of installed families as an argument, the view supplies it), `editor.font_size`
   (10 to 24 in steps of 1, default 13; the schema allows 6 to 72 and a value outside 10..=24 is shown as is
   and kept), `editor.font_ligatures` (default false). "Default (<family>)" is the first option and removes
   the key.
3. **Keyboard**: Standard (Tenmoku's default VS Code-style keys) or Vim, i.e. `vim.enabled`. With Vim chosen
   only: `vim.preset` (`none` "Plain vim" / `lazyvim` "LazyVim, Space as leader") and `editor.line_numbers`
   (`on` / `relative` / `hybrid`; `off` isn't offered). There is no base-keymap setting (VS Code / JetBrains /
   Sublime) in Tenmoku today, so none is offered.
4. **Editing**: `editor.format_on_save` (false), `editor.auto_save` as Off / After a pause / When focus leaves
   (`off`, `after_delay`, `on_focus_change`; a current value of `on_window_change` or `on_insert_leave` shows as
   a fourth, "Custom", option that writes nothing), `editor.inline_diagnostics` as Off / Errors / Errors and
   warnings (`off`, `error`, `warning`; `all` shows as "Custom" as above), `editor.minimap.enabled` (true),
   `editor.inlay_hints` (true), `editor.soft_wrap` (false).
5. **Ready**: no settings; a summary of how many settings the run changed, and where to change them later.

Left out on purpose, so the wizard stays five short pages: spell check (`editor.spell_check` doesn't exist
yet, T1332), restoring the session (no setting: hot exit is always on), telemetry (none), update checks
(manual only, T1309), the AI chat (it has its own first-use notice and key flow), UI density (no setting).

### What the model gives

Suggested shape (names may differ; the behaviour is the contract):

- `Step` (the five above) and `steps()`; each step's `controls(&Choices)`: a control has its dotted key, a
  title and one-line description, and its options, each with a label. Every title, description and label is
  the text the view shows and also its VoiceOver label, so none is empty. Controls hidden by another choice
  (the Vim-only two, the second theme list) aren't listed.
- `Choices::from_settings(&fe_config::Settings, installed_fonts: &[&str])`: what each control shows now.
- `Choices::choose(control, option) -> Vec<(Vec<&'static str>, Option<fe_config::SettingValue>)>`: the
  writes that one choice makes, for `fe_config::write_settings`. A value equal to `Settings::default()`'s
  removes the key (`None`); choosing what's already shown writes nothing; changing `theme.mode` writes only
  the mode; a theme writes its slot and, if set, removes `theme.name`.
- `Choices::changed_count()`: how many keys the run changed from what it started with (for Ready).

## Criteria notes

- Writes go through `fe_config::write_settings` to an explicit path (the tests' temp file), which keeps the
  file's comments and creates a missing file from `USER_SETTINGS_TEMPLATE`. The model never reads the
  process's config dir.
- Every key the model can write must exist in `fe_config::settings_schema()`; test it by loading the written
  file with `fe_config::SettingsStore::load(Some(path), None)` and asserting no diagnostics.

## Implementation notes

- `editor.font_size` is written as a float (`15.0`), as `settings-template.toml` spells it, so it takes the place of
  the template's commented-out default rather than sitting beside it.
- `theme.name` is removed only when choosing a theme for the slot of its appearance, or when it names no bundled theme.
  Choosing a light theme with `theme.name = "Ember"` keeps the name, since removing it would change the dark theme
  behind the user's back.
- A value a control doesn't offer is listed while current and writes nothing when chosen: a theme or font by its own
  name, a size as a number, any other (`editor.line_numbers = "off"` too) as "Custom". Once left it isn't offered, as
  nothing could write it back.
