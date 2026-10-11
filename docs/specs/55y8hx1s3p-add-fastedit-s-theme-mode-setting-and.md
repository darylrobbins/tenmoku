---
task_uid: "01m4f8qn143yxnc955y8hx1s3p"
task: "T245"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test theme_setting::theme_mode_key: Settings::default().theme.mode is ThemeModeSetting::Dark; a user file with [theme] mode = \"system\" sets System; mode = \"blue\" is an Error diagnostic and leaves Dark; schema_matches_golden and template_is_all_defaults pass"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test theme_setting::project_file_cannot_set_the_theme: a project file with [theme] mode = \"light\" over a user file with mode = \"system\" leaves System, with one Warning at theme.mode whose message is PROJECT_THEME"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test editor_palette::bundled_palettes: EditorPalette::for_theme(ThemeData::dark()) equals EditorPalette::fastedit_dark(), whose editor.background is #16181d, cursor #7aa2f7, popover_background #21252b and keyword #c678dd; for_theme(ThemeData::light()) equals fastedit_light(), whose editor.background equals ThemeData::light()'s surface.background token; both bundled files parse with no diagnostics, set all 13 editor keys and list the same 25 captures"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test editor_palette::extension_tables_override: a light theme parsed with fe_config::theme::EXTENSIONS, with [editor] background = \"#fafafa\" and cursor = 3 and [syntax] keyword = \"#d00000\" and \"function.method\" = \"#123456\", gives fastedit_light() with background #fafafa, keyword #d00000 and function.method appended last, cursor unchanged, and exactly one warning, naming editor.cursor"
    required: true
    manual: false
  - id: "C5"
    text: "rg -nw 'ThemeStore|ThemeSet|load_themes|themes_dir|UiColors' crates/ (whole words, so ThemeSettings doesn't match) finds no match, and crates/fe-config/src/theme_store.rs, crates/fe-config/tests/theme_store.rs and crates/fe-config/tests/theme.rs no longer exist"
    required: true
    manual: false
---

# Add fastedit's theme mode setting and editor palettes read from a kiln theme's editor and syntax tables, and retire fe-config's ThemeStore

## Problem

The user asked once T238 landed (plan approved 2026-10-08) for "a theme setting", the selected theme's
colours "in place of today's hard-coded ones", and "hot reload when the theme file or the setting changes".
T238 and T244 built that in fe-config (`Theme`, `ThemeSet`, `load_themes`, `ThemeStore`), but nothing uses
it, and kiln (K3–K5, on main at e6bab88) now provides the same machinery:

- kiln-ui's `GlobalTheme` (`cx.theme()`), a dark and light pair with a `ThemeMode` (Dark, Light, System)
  picking between them (`set_themes`, `set_theme_mode`, `follow_system_appearance`):
  `kiln/crates/kiln-ui/src/theme.rs`.
- kiln-core's theme files: tokens, the `[ui]` alias table fastedit's chrome colours use, and an app's own
  tables kept as `ThemeData::extensions` (`kiln/crates/kiln-core/src/theme/{mod,file}.rs`).
- `kiln::Config::user_theme(path)`: one user theme file, hot-reloaded by kiln's config service, laid over
  the dark or light base as its `appearance` says (`kiln/crates/kiln/src/{setup,config}.rs`).

**Decided** (user, 2026-10-11): "Use kiln's theme service". Theme selection and hot reload in fastedit go
through kiln's `GlobalTheme`, `ThemeMode`, the hot-reloaded user theme file and kiln's config service, not
fe-config's `ThemeStore` and `ActiveUiTheme`.

So fastedit no longer selects a theme by name from a themes directory: kiln has one user theme file and a
mode. What fastedit still owns, and kiln can't, is its editor's colours (7 editor colours, 6 diagnostic and
popover colours, 25 syntax captures, hard-coded in `crates/fe-gpui/src/lib.rs`, `EditorTheme::from_settings`)
and the settings key that picks the mode. This card is that GPUI-free half, in fe-config. The next card
(fe-gpui and the host) applies it.

## Change

**Setting.** In `crates/fe-config/src/settings.rs`, add `theme: ThemeSettings` to `Settings` (`#[serde(default)]`,
doc "The colour theme"), with `pub struct ThemeSettings { pub mode: ThemeModeSetting }` and
`pub enum ThemeModeSetting { #[default] Dark, Light, System }`, serialized lower case. `mode`'s doc: "dark,
light, or system to follow the system's appearance. Your own colours go in theme.toml beside this file; its
appearance says whether it replaces the dark or the light theme." Dark keeps today's look.

- The theme is the app's, not a window's (kiln's `GlobalTheme` is one global), so a project file may not set
  it: add `ProjectFilter::new("theme", drop_theme)` to `options()` beside the `lsp` filter. `drop_theme`
  removes the `theme` table from a project layer with a warning at each key it set, message
  `pub const PROJECT_THEME: &str = "theme settings in project files are ignored: the theme is the same in every window"`
  (as `narrow_lsp` places `UNTRUSTED_LSP`).
- Regenerate `crates/fe-config/settings.schema.json` (`UPDATE_GOLDEN=1`, checked by `schema_matches_golden`)
  and add a `[theme]` section with a commented `# mode = "dark"` and its doc line to
  `crates/fe-config/settings-template.toml` (`template_is_all_defaults`).
- Export `ThemeSettings` and `ThemeModeSetting` from `crates/fe-config/src/lib.rs`.

**User theme path.** In `crates/fe-config/src/paths.rs`, `pub fn user_theme_path() -> Option<PathBuf>`:
`<config_dir>/theme.toml`, beside `settings.toml`. Remove `themes_dir` and `themes_dir_from` (only the
retired `load_themes` used them).

**Editor palette.** Rewrite `crates/fe-config/src/theme.rs` around kiln-core's theme data (fe-config already
depends on kiln-core):

- `pub use kiln_core::theme::Color;` replaces fe-config's own `Color`.
- `pub const EXTENSIONS: &[&str] = &["editor", "syntax"];` the app tables a fastedit theme file has, for
  `ThemeFile::load`/`parse` and, in the next card, kiln's user theme.
- `EditorColors` (the `colors!` table) gains `error`, `warning`, `information`, `hint`, `popover_background`
  and `popover_border` after its 7 keys, so every colour `EditorTheme` has is themeable.
- `pub struct EditorPalette { pub editor: EditorColors, pub syntax: Vec<(String, Color)> }` (the `syntax` list
  keeps T238's rules: a capture takes its longest listed prefix's colour; the base's entries first, then the
  theme's new ones in table order; no name twice).
  - `EditorPalette::fastedit_dark() -> &'static EditorPalette` and `fastedit_light()`: the bundled palettes,
    parsed once from `crates/fe-config/themes/fastedit-dark.toml` and the new
    `crates/fe-config/themes/fastedit-light.toml`. Each file holds only `name`, `appearance`, `[editor]` with
    all 13 keys and `[syntax]` with the same 25 captures. Dark's values are exactly today's
    `EditorTheme::from_settings` literals (move the diagnostic and popover ones in; drop its `[ui]` table:
    kiln's Kiln Dark base has the same chrome colours). Light's `editor.background` is Kiln Light's
    `surface.background` (`#ffffff`), and its other colours are a readable light palette of the
    implementer's choosing.
  - `EditorPalette::for_theme(theme: &kiln_core::theme::ThemeData) -> (EditorPalette, Vec<String>)`: the
    bundled palette for `theme.appearance`, with `theme.extension("editor")` and `extension("syntax")` laid
    over it. A key that isn't a colour key or capture name, or a value that isn't a colour string, is left
    out with one warning naming its dotted key (`editor.cursor: expected a colour string, found integer`);
    the extension tables carry no line numbers, so the warnings don't either.
- Delete the rest of T238's parser and catalog: `Theme`, `UiColors`, `Appearance`, `ThemeSet`,
  `load_themes`, `FASTEDIT_DARK_SOURCE`. Delete `crates/fe-config/src/theme_store.rs` (T244's `ThemeStore`,
  `ThemeEvent`) and its tests `crates/fe-config/tests/theme_store.rs`, and `crates/fe-config/tests/theme.rs`,
  which tests only the deleted API. Update `crates/fe-config/src/lib.rs`'s exports and module doc.
  Nothing outside fe-config uses any of them (checked on e6bab88).

**Decision record.** Add a short section to `docs/decisions/app-framework.md`, "8. Theming in fastedit",
recording the user's decision and the split: kiln owns theme files, the dark/light pair, the mode, the user
theme file and its hot reload; fastedit owns its `[theme] mode` setting, its `editor`/`syntax` extension tables
with the bundled dark and light palettes, and building `EditorTheme` from them. It supersedes
`docs/decisions/app-shell.md` §4.4.

Not in this card: anything with GPUI. fe-gpui, fastedit's host and the workbench are the next two cards.

## Tests

- `crates/fe-config/tests/theme_setting.rs`: a new file, for the setting.
- `crates/fe-config/tests/editor_palette.rs`: a new file, for `EditorPalette`. Build themes with
  `kiln_core::theme::{ThemeFile, ThemeData}` from TOML strings, parsed with `fe_config::theme::EXTENSIONS`.

## Implementation notes

- C5 was respecced to a whole-word search (`rg -nw`): as a substring
  search, `ThemeSet` also matched the `ThemeSettings` this card adds.
- Warnings from `EditorPalette::for_theme` are `<dotted key>: <reason>`:
  `expected a colour string, found integer`, kiln's colour parse error,
  `unknown key`, or `not a capture name: …`.
- A theme's capture that the base lists keeps its place with the new
  colour; only new captures are appended.
- `EditorPalette::bundled(appearance)` picks the bundled palette;
  `paths::THEME_FILE` names `theme.toml`; kiln's `THEMES_DIR` is no longer
  re-exported.
- fastedit Light: Kiln Light's background, status and border colours for
  the editor, diagnostics and popovers, and One Light's syntax colours.
