---
task_uid: "01m57zd9bkg2mnvxa94q0v4b47"
task: "T202"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test bundled_themes::finds_by_title_or_id: find(\"Dark Modern\"), find(\"vscode-dark-modern\") and find(\" dark modern \") return the Dark Modern entry, find(\"Ember\") the Ember one, find(\"nope\") None; bundled_themes::every_entry_parses: each registry file parses with no diagnostics under EXTENSIONS and its appearance matches the file's"
    required: true
    manual: false
  - id: "C2"
    text: "kiln test builtin_themes::installed_builtin_themes_survive_a_rescan: after install_builtin_themes, themes(cx) lists them and a rescan of the extensions directory keeps them"
    required: true
    manual: false
  - id: "C3"
    text: "fastedit unit test theme_name_tests::name_selects_a_bundled_theme_live: with settings theme.name = \"Dark Modern\" the dark slot's editor palette is Dark Modern's; changing it to \"Fastedit Ember\" on a settings reload re-selects with no restart; an unknown name leaves the built-in and publishes a warning diagnostic; theme.dark set too wins for that slot"
    required: true
    manual: false
  - id: "C4"
    text: "ThemeSettings.name is documented in settings-template.toml and ignored in a project settings file with the existing project-theme warning; cargo nextest run -p fe-config -p kiln -p fastedit passes with no existing test edited; cargo clippy -p fe-config -p kiln -p fastedit --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Select a bundled theme by name with [theme] name, so "Dark Modern" and Ember need no copied theme.toml (Mac T997)

## Problem

The user asked (2026-10-18) for bundled themes selectable by name, "a setting like `theme = "Dark Modern"`".
Today fastedit bundles `fastedit-dark.toml`, `fastedit-light.toml` and `fastedit-ember.toml` in
`crates/fe-config/themes/`, and T994 adds `vscode-dark-modern.toml`. Only the dark and light ones are reachable:
`EditorPalette::bundled(appearance)` (`crates/fe-config/src/theme.rs`) picks by appearance, and Ember (and Dark
Modern) would be a file the user copies to `theme.toml`. The one way to name a theme is `[theme] dark = "<id>"` /
`light = "<id>"` (`ThemeSettings`, `crates/fe-config/src/settings.rs`), which names an installed extension theme
`<extension id>/<file stem>` through `kiln::extensions::select_themes` (`kiln/crates/kiln/src/extensions.rs`; the
layers are `Layers`/`Installed` in `kiln/crates/kiln/src/config.rs`, applied by `crates/fastedit/src/theme.rs`).

A top-level `theme = "..."` key cannot exist: `theme` is already the `[theme]` table. The key is `[theme] name`.

## Change

`[theme] name = "Dark Modern"` selects a bundled theme by its title.

1. New `crates/fe-config/src/themes.rs` (declared in `crates/fe-config/src/lib.rs`): the registry of bundled themes,
   a `const` table of `(title, id, appearance, include_str!("../themes/<file>.toml"))` for Dark Modern
   (`vscode-dark-modern`), Fastedit Dark, Fastedit Light and Fastedit Ember; `pub fn bundled_themes() ->
   &'static [BundledTheme]` and `pub fn find(name: &str) -> Option<&'static BundledTheme>`, matching the title or
   the id, ignoring case and surrounding space. The titles are this table's, so "Dark Modern" works whatever the
   file's own `name` is.
2. `kiln/crates/kiln/src/extensions.rs` + `config.rs`: `pub fn install_builtin_themes(cx: &mut App, themes:
   &[(&str, &ThemeFile)])` adds themes to `Layers::installed` under IDs in a reserved namespace
   (`builtin/<id>`), kept across extension rescans, listed by `themes(cx)` like any installed theme.
3. `crates/fe-config/src/settings.rs`: `ThemeSettings` gains `name: Option<String>` (user file only, as `mode`,
   `dark` and `light`: a project file's is ignored with the existing project-theme warning), documented in
   `crates/fe-config/settings-template.toml` as a commented-out line. `crates/fastedit/src/theme.rs` `init`
   installs the registry's themes at start, then resolves the selection: `name` fills the slot of the found
   theme's appearance, unless `dark`/`light` is also set for that slot, which wins (a diagnostic says `name` is
   ignored for it). An unknown `name` is a warning diagnostic ("theme Dark Moderrn isn't bundled") and the slot
   keeps the built-in. A change of `name`, `dark` or `light` on a settings reload re-selects live.
4. Tests, new files only: `crates/fe-config/tests/bundled_themes.rs` and `kiln/crates/kiln/tests/builtin_themes.rs`
   (registry and install), and a test in a new `crates/fastedit/src/theme_name_tests.rs` declared from
   `crates/fastedit/src/theme.rs`.

Not in this card: the command-palette picker (its own card, which depends on this one).

## Implementation notes

- `find` matches a title, the title without its "Fastedit " prefix, or the ID, so C1's `find("Ember")` (and
  "Dark", "Light") work beside "Fastedit Ember".
- kiln keeps the app's themes in their own map (`ThemeLayers::builtin`), beside the extensions' `installed`,
  which a rescan replaces; `themes(cx)` lists both in ID order. `builtin/<id>` can't clash with an extension's
  theme, since an extension ID has two or more segments. `builtin_theme_id` and `BUILTIN_THEMES` name the
  namespace.
- The warnings about `theme.name` need to outlast the config service's batches, which republish only the
  sources' diagnostics. So kiln gained `select_themes_noting(cx, dark, light, notes)`: the app's notes become
  part of the selection's diagnostics (`SelectionSource`), as kiln's own "isn't installed" warnings are.
  `select_themes` is that with no notes.
- The warnings point at the user's `settings.toml` (`fe_config::user_settings_path`), at the `name` key's line
  in the text the settings were last accepted from. An overridden name gets "theme.name Dark Modern is
  ignored: theme.dark chooses the dark theme".
- Outside the listed scope: `crates/fe-config/settings.schema.json` is regenerated (`UPDATE_GOLDEN=1`, checked
  by `schema_matches_golden`) for the new key. `fastedit-ember.toml`'s header still says to copy it to
  `theme.toml`; that's the theme file's text, left for a later edit.
