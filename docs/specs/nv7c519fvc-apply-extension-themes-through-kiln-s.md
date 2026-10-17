---
task_uid: "01m55c0y73tb27jtnv7c519fvc"
task: "T142"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extension_themes_are_listed_and_selected: kiln::extensions::themes lists \u003cid\u003e/\u003cstem\u003e with title and appearance, and select_themes makes the dark slot the extension's theme (its tokens over the bundled base)"
    required: true
    manual: false
  - id: "C2"
    text: "the_user_theme_lays_over_the_selected_theme: a user theme.toml setting one token keeps every other token of the selected extension theme, not the bundled base's"
    required: true
    manual: false
  - id: "C3"
    text: "a_missing_or_mismatched_selection_falls_back: an unknown theme ID gives the bundled theme and a warning diagnostic, a light theme selected for the dark slot is a warning and unused, and editing the selected theme's file then sending a batch re-applies it"
    required: true
    manual: false
  - id: "C4"
    text: "theme_names_key: [theme] dark and light parse from the user file, a project file's are ignored with the PROJECT_THEME warning, and template_is_all_defaults still passes"
    required: true
    manual: false
  - id: "C5"
    text: "theme::tests::theme_names_follow_settings (crates/fastedit/src/theme.rs): changed theme.dark in reloaded settings reaches select_themes; the_shipped_config_is_clean still passes; scripts/check_kiln_boundary.py passes"
    required: true
    manual: false
---

# Apply extension themes through kiln's theme layer, chosen by name in settings, with the user's theme.toml laid over the chosen theme (Mac T842)

Filed on argon from Mac T842 (card ~/coordination/argon-cards/zzn/T842.json rev 1; note zzn, kiln extensions stage 1: declarative, data only). Mac task uid 01m55bz4rpahexrp1sh5eaqw69. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Respect scripts/check_kiln_boundary.py and give every new kiln file its SPDX header line. Argon deps: T141 (Mac T841). Title the host commit "... (Mac T842)".

## Problem

Stage-1 extensions contribute themes (`contributes.themes`, extensions record §2, §4), but kiln
has one user theme file and a mode: "There's no themes directory or picker by name"
(`app-framework.md` §8, which the extensions record amends). `ThemeData::from_file` always lays
a file over the bundled base (`kiln/crates/kiln-core/src/theme/mod.rs` ~89), and `UserTheme`
puts the user's file into the dark or light slot over it (`kiln/crates/kiln/src/config.rs`).

## Change

Precedence, lowest first: the bundled base, the selected extension theme, the user's
`theme.toml`.

- `kiln/crates/kiln-core/src/theme/mod.rs`: `ThemeData::from_file_over(file: &ThemeFile, base:
  &ThemeData) -> ThemeData`; `from_file` becomes `from_file_over(file, Self::base(appearance))`.
- `kiln/crates/kiln/src/extensions.rs`: each `contributes.themes` file is read with
  `ThemeFile::load(path, config.theme_extensions)` and laid over the bundled base of its
  appearance; its ID is `<extension id>/<file stem>` and its title the file's `name`.
  Problems are the extension's diagnostics (warnings; a file that doesn't load is an error for
  that theme only). `kiln::extensions::themes(cx) -> Vec<ThemeInfo { id, title, appearance }>`
  lists them, in id order.
- `kiln::extensions::select_themes(cx, dark: Option<&str>, light: Option<&str>)`: the theme
  each slot of the pair starts from: the extension theme with that ID, else the bundled one
  with a warning diagnostic ("theme acme.solarized/solarized-dark isn't installed"). A theme
  whose appearance doesn't match its slot is a warning and not used.
- `kiln/crates/kiln/src/config.rs`: `UserTheme` lays the user's file over the slot's selected
  theme (`from_file_over`), not the bundled base, and re-lays it when the selection or the
  extension's file changes. A rescan that changes a selected theme's file re-applies it
  (`set_themes`), so editing an installed theme shows at once.
- fastedit: `crates/fe-config/src/settings.rs` `ThemeSettings` gains `dark: Option<String>`
  and `light: Option<String>` (user file only, as `mode`: a project file's is ignored with the
  existing `PROJECT_THEME` warning), documented in `crates/fe-config/settings-template.toml`
  as commented-out lines. `crates/fastedit/src/theme.rs` `init` passes them to
  `select_themes` at start and whenever they change on a settings reload (observing
  `GlobalSettings<Settings>` there, not in `main.rs`, which T410 holds).
- fastedit turns extensions on: `crates/fastedit/src/config.rs` gains
  `pub(crate) fn with_extensions(config: Config, dir: PathBuf) -> Config`
  (`Config::extensions(dir, &fe_config::paths::app_id().short, ApiVersion { major: 1, minor: 0 })`:
  manifests name `[engine] fastedit = "1.0"`),
  which `config()` calls with `fe_config::paths::app_paths().extensions_dir()`. Later cards add
  fastedit's own kinds (`lsp`, `grammars`) there.

Every new kiln file starts with the SPDX line and nothing in `kiln/` names fastedit
(`scripts/check_kiln_boundary.py`). Tests: `kiln/crates/kiln/tests/extension_themes.rs`
(headless, an extension written to a `tempfile` dir, reloads sent as batches),
`crates/fe-config/tests/theme_names_setting.rs`.

## Out of scope

A theme picker UI (a later card can list `themes(cx)` in the palette); icon themes.

## Implementation notes

- The pair's layers live in one shared `Layers` global (`kiln/crates/kiln/src/config.rs`), fed by
  the user theme source and the extension source and read by `select_themes`. Applying keeps the
  old `UserTheme` rule: a slot the layers don't touch keeps whatever the app put there.
- A selection's warnings come from a `SelectionSource`, added last to the config service so batch
  publishes include them; `select_themes`, which runs between batches, swaps its old warnings for
  the new ones at the end of `ConfigDiagnostics`. They name the extensions directory as their file.
- An extension theme file with errors keeps its last good version while the file exists, as a
  manifest does. `ThemeData::from_file_over` merges app tables (`[editor]`, `[syntax]`) key by
  key, the file's over the base's.
- `kiln::extensions::themes` takes `&App`.
