---
task_uid: "01m4f642fbwfxk17kkh36bsa4q"
task: "T238"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test theme::bundled_dark_matches_the_hard_coded_colours: the bundled fastedit Dark theme has exactly the 20 UiTheme::default_dark colours (crates/fe-ui/src/theme.rs) and the 7 editor and 25 syntax colours of EditorTheme::from_settings (crates/fe-gpui/src/lib.rs), checked against a literal table in the test"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test theme::partial_theme_falls_back: a user theme setting only editor.background and syntax.\"keyword\" gets every other colour from fastedit Dark, with no diagnostics"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test theme::bad_values_are_diagnosed: an unknown key is a Warning; a malformed colour (\"#12345\", \"red\") is an Error whose line and column point at the value, and that key falls back while the rest of the file applies; a TOML syntax error leaves the theme out of the ThemeSet"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test theme::themes_dir_lists_user_themes: load_themes over a themes dir reads every *.toml, a user theme named like a bundled one overrides it, get() ignores case, and #rgb, #rrggbb and #rrggbbaa all parse"
    required: true
    manual: false
---

# Add GPUI-free themes to fe-config: theme TOML files with UI, editor and syntax colours, a bundled fastedit Dark matching today's colours, and diagnostics

## Problem

Milestone M5 in the plan (`~/.claude/plans/glittery-foraging-graham.md`) is "Config, keymaps, themes, hot reload".
Settings and keymaps are done (T71, T72, T73), but themes aren't. Every colour is hard-coded, and both places say
"hard-coded until themes land (M5)":

- `crates/fe-ui/src/theme.rs`: `UiTheme::default_dark` has 20 chrome colours.
- `crates/fe-gpui/src/lib.rs`: `EditorTheme::from_settings` has 7 editor colours, plus 25 syntax capture colours
  passed to `fe_syntax::resolve_theme`.

Core crates must not depend on GPUI. So the theme *model* (file format, parsing, fallbacks, diagnostics,
discovery) belongs in `fe-config` next to the settings layers, and turning it into `Hsla` belongs in the UI
crates later.

## Change

New module `crates/fe-config/src/theme.rs`, exported from `crates/fe-config/src/lib.rs`. It has no GPUI and no new
dependencies (`toml`, `serde` and `schemars` are already there).

- `Color { r: u8, g: u8, b: u8, a: u8 }` parsed from `"#rgb"`, `"#rrggbb"` or `"#rrggbbaa"`. Nothing else is
  accepted: no names and no `rgb()`.
- `Theme { name: String, appearance: Appearance (Dark | Light), ui: UiColors, editor: EditorColors, syntax: Vec<(String, Color)> }`.
  - `UiColors` has one field per `UiTheme` field, with the same names.
  - `EditorColors` has `background`, `foreground`, `gutter_foreground`, `gutter_active_foreground`,
    `active_line`, `selection` and `cursor`.
  - `syntax` is keyed by dotted capture names (`"keyword"`, `"function.macro"`) in file order. Keys are validated
    lexically only: dot-separated `[a-z0-9_]` segments. fe-config doesn't depend on fe-syntax.
- File format: top-level `name` and `appearance`, then `[ui]`, `[editor]` and `[syntax]` tables of colour strings.
  A theme may be partial. A missing key takes its value from the bundled **fastedit Dark** theme, with no
  diagnostic. Its `[syntax]` entries replace the same keys in fastedit Dark and add new ones.
- Bundled theme: `crates/fe-config/themes/fastedit-dark.toml`, compiled in with `include_str!`. Its values are
  exactly today's hard-coded colours from the two files above. Copy the hex literals, and keep the alpha from the
  `rgba(...)` ones.
- Parsing reuses `layers::load_layer` (or its parse half) so diagnostics carry the file, the dotted key, and the
  line and column of the offending value, like settings diagnostics:
  - an unknown key or table is a `Severity::Warning`, and the key is ignored;
  - a malformed colour or a wrong type is a `Severity::Error` on that key, and the key falls back as if missing;
  - a TOML syntax error rejects the file, and the theme isn't listed.
- Discovery: `paths::themes_dir()` returns `<config_dir>/themes`, with a `themes_dir_from(env)` twin like
  `config_dir_from`. `load_themes(dir: Option<&Path>) -> (ThemeSet, Vec<Diagnostic>)` reads every `*.toml` in the
  directory, sorted by file name. `ThemeSet::get(name)` looks names up case-insensitively, and a user theme
  overrides a bundled theme of the same name. `ThemeSet::names()` lists them for a future picker.
  `ThemeSet::default()` is the bundled theme alone.

Not in this card, so the lead can file them once the files are free:

- a `theme = "<name>"` setting (`settings.rs` and the schema are in T223's scope);
- converting to `UiTheme`/`EditorTheme` and applying the theme (fe-ui, fe-gpui `lib.rs`);
- hot reload of the themes directory;
- a theme picker command.

## Tests

All tests go in the new file `crates/fe-config/tests/theme.rs`, using `tempfile` directories.
