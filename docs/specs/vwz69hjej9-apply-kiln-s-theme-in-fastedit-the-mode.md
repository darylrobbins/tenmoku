---
task_uid: "01m4pzqnd68nwsbjvwz69hjej9"
task: "T35"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test theme_tests::from_settings_is_fastedit_dark: EditorTheme::from_settings(\u0026EditorSettings::default(), cx) equals EditorTheme::from_palette(EditorPalette::fastedit_dark(), \u0026EditorSettings::default(), cx), and editor_palette(cx) is fastedit Dark before set_editor_palette and the set palette after"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test theme_tests::set_colors_keeps_metrics: EditorView::set_colors(fastedit_light()) changes theme().background, popover_background and the keyword highlight's syntax_color and leaves font_family, font_size and line_height unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "rg -n 'rgba?\\(0x' crates/fe-gpui/src/lib.rs finds no match"
    required: true
    manual: false
  - id: "C4"
    text: "fastedit test theme::tests::palette_follows_kiln: after theme::init, kiln set_themes(ThemeMode::Light, Theme::dark(), Theme::light()) (kiln's own themes, with no editor or syntax table) makes fe_gpui::editor_palette(cx) fastedit Light, set_theme_mode(Dark) makes it fastedit Dark, and set_theme with a dark theme whose [editor] background = \"#000000\" (parsed with EXTENSIONS) gives that background with fastedit Dark's other editor colours; theme::mode maps Dark, Light and System to kiln's"
    required: true
    manual: false
  - id: "C5"
    text: "main.rs passes the settings' theme.mode to Config::theme_mode, user_theme_path() to Config::user_theme, and fe_config::theme::EXTENSIONS to Config::theme_extensions; calls theme::init after kiln::init, builds each window's EditorTheme with theme::editor_theme, calls follow_system_appearance in new_workbench, and calls set_theme_mode when a reload changes theme.mode; theme::init refreshes set_restore_style on each change; nothing under kiln/ changes (the source lines quoted in the decision file)"
    required: true
    manual: false
---

# Apply kiln's theme in fastedit: the mode setting and user theme file through kiln's config, and editor colours from the active kiln theme for each window and after each change (Mac T246)

Filed on argon from Mac T246 (card ~/coordination/argon-cards/T246.json rev 4, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-h.md). Mac task uid 01m4f8qn9qh9wh4vrgw1gk9yen. Its dependencies (Mac T245, Mac T223, Mac T302, Mac T354) are already on argon main (synced to mac/main 6d07f26), so none are recorded here. Title the host commit "... (Mac T246)".

## Problem

After the fe-config card (T245), fastedit has a `[theme] mode` setting, a user theme file path
(`fe_config::paths::user_theme_path()`), and `fe_config::theme::EditorPalette`, which turns a kiln theme into
editor colours. Nothing applies them:

- `crates/fastedit/src/main.rs`, `config()` (line 87): kiln is set up with `.theme_mode(ThemeMode::Dark)` and
  no user theme file.
- `crates/fe-gpui/src/lib.rs`, `EditorTheme::from_settings` (line 107): every editor colour is a hex literal.
  `main.rs` builds each window's `EditorTheme` with it (`open_window_on`, line 245, and the launch windows,
  line 422), and gives it to restored editors through `fe_gpui::set_restore_style` (line 423).

The user asked for the selected theme's colours "in place of today's hard-coded ones" and "hot reload when
the theme file or the setting changes" (plan approved 2026-10-08).

**Decided** (user, 2026-10-11): "Use kiln's theme service". Theme selection and hot reload go through kiln's
`GlobalTheme` (`cx.theme()`), `ThemeMode`, the hot-reloaded user theme file and kiln's config service, not
fe-config's `ThemeStore` and `ActiveUiTheme`. kiln already loads, watches and reloads the user's theme file,
keeps the last good version on an error, picks dark or light by the mode, follows the system's appearance and
redraws every window on a change (`kiln/crates/kiln-ui/src/theme.rs`, `kiln/crates/kiln/src/config.rs`). This
card doesn't rebuild any of that: it gives kiln fastedit's settings and file, and derives the editor's colours
from kiln's active theme.

**kiln's side is on main** (871f205, 2026-10-11): `kiln::Config::theme_extensions(names: &'static [&'static
str])` (`kiln/crates/kiln/src/setup.rs`) names an app's tables in the user's theme file. `UserTheme` passes them
to `ThemeFile::load` on every read, so `cx.theme().data().extension("editor")` and `extension("syntax")` hold
the tables as written, and a change to one of them alone reloads and applies the theme. It panics on a name
kiln reserves (`ThemeFile::is_reserved`: `ui`, the token groups `surface`, `text` …); `editor` and `syntax`
aren't reserved. kiln's own themes have no extension tables. This card changes nothing under `kiln/`.

**Fallback** (user, 2026-10-11): when the active theme has no `editor` table (`extension("editor")` is
`None`), the editor uses fastedit's bundled palette for the theme's appearance. That covers kiln's own Kiln
Dark and Kiln Light, the appearance the user's file doesn't replace, and a user file with no `[editor]`.
T245's `EditorPalette::for_theme` already does this: it starts from `fastedit_dark()` or `fastedit_light()` by
`theme.appearance` and lays only the tables present over it, `editor` and `syntax` each on its own. The host
uses its result as it is. It never derives editor colours from kiln's chrome tokens, and never keeps the
previous palette.

## Change

**fe-gpui** (`crates/fe-gpui/src/theme.rs`, a new module, declared from `lib.rs`):

- `EditorTheme::from_palette(palette: &EditorPalette, settings: &EditorSettings, cx: &App) -> EditorTheme` and
  `EditorTheme::set_colors(&mut self, palette: &EditorPalette)`, which sets the 13 colours from
  `palette.editor` and `syntax` from `fe_syntax::resolve_theme` over `palette.syntax` in its order, through
  `gpui::rgba(color.to_rgba_u32())`, and leaves the metrics alone.
- `impl EditorView { pub fn set_colors(&mut self, palette: &EditorPalette, cx: &mut Context<Self>); pub fn
  theme(&self) -> &EditorTheme }` (an inherent impl block here, not in `view.rs`): `set_colors` calls
  `self.theme.set_colors` and notifies.
- The editor colours in use, as a GPUI global set by the host: `pub struct ActivePalette`,
  `pub fn set_editor_palette(cx: &mut App, palette: Arc<EditorPalette>)` and
  `pub fn editor_palette(cx: &App) -> Arc<EditorPalette>` (fastedit Dark when unset). Export them from `lib.rs`.
  fe-gpui stays free of kiln-ui: the host turns kiln's theme into a palette.
- In `crates/fe-gpui/src/lib.rs`, `from_settings(settings, cx)` becomes
  `from_palette(EditorPalette::fastedit_dark(), settings, cx)`, removing every hex literal, and the
  `EditorTheme` doc stops saying the colours are "hard-coded until themes land (M5)".
- `crates/fe-gpui/src/pane_item.rs` doesn't change (T221 is editing it): the host refreshes the restore
  style instead (below).

**Host** (`crates/fastedit/src/theme.rs`, a new module of the binary):

- `fn mode(setting: ThemeModeSetting) -> kiln::ui::ThemeMode`.
- `fn init(cx: &mut App)`: sets `fe_gpui::set_editor_palette` from
  `EditorPalette::for_theme(cx.theme().data())` now, and again from a `cx.observe_global::<GlobalTheme>`
  observer on every change. On each change it also calls `fe_gpui::set_restore_style` with
  `editor_theme(&settings.editor, cx)` and the editor settings in use (`GlobalSettings<Settings>`), so editors
  restored from a saved layout after a change open in the new colours, and it logs the palette's warnings
  (target `fastedit`, with the theme's name).
- `fn editor_theme(settings: &EditorSettings, cx: &App) -> EditorTheme`:
  `EditorTheme::from_palette(&fe_gpui::editor_palette(cx), settings, cx)`.

**Host** (`crates/fastedit/src/main.rs`):

- `config()` takes the settings' mode: `.theme_mode(theme::mode(settings.theme.mode))`, plus
  `.user_theme(path)` when `fe_config::paths::user_theme_path()` is `Some`, and
  `.theme_extensions(fe_config::theme::EXTENSIONS)` (kiln's, from 871f205). Update its doc list ("kiln's dark theme, which fastedit's
  chrome has always had").
- `theme::init(cx)` right after `kiln::init`.
- Each window's `EditorTheme`, at launch and in `open_window_on`, comes from `theme::editor_theme`, and so does
  `set_restore_style`'s.
- `new_workbench` calls `window.follow_system_appearance(cx)` (`kiln::prelude::WindowExt`), so `mode = "system"`
  follows the system live: fastedit's windows don't use kiln's `Root`, which is what calls it otherwise.
- `follow_settings`' observer: when the reload changed `theme.mode`
  (`global.settings().theme.mode != global.previous().theme.mode`), `cx.set_theme_mode(theme::mode(..))`.

Not in this card: open editors and fe-workbench's own views (panes' toolbars, panels, pickers, the status bar)
following a change live. The next card (T247) does that. Until it lands, a change reaches new windows,
restored editors, kiln-ui's widgets, and kiln-shell's shell, dock, panes, pane borders, picker and command
palette. Those kiln-shell views read the active theme as they draw since ba016a0. Also until T247:
`Workbench::reload_settings` (`crates/fe-workbench/src/workbench.rs`, line 1426) resets `set_restore_style`
from the active pane's `EditorTheme` on every settings reload. So a settings reload after a theme change
puts restored editors back in the old colours. T247's panes follow the palette, which fixes that.

## Tests

- `crates/fe-gpui/src/theme_tests.rs`: a new file, with one `#[cfg(test)] mod theme_tests;` line in
  `crates/fe-gpui/src/lib.rs`.
- The host's tests go in `crates/fastedit/src/theme.rs`'s own `#[cfg(test)] mod tests`, as `perf.rs` keeps
  its own. They need `gpui = { workspace = true, features = ["test-support"] }` as a dev-dependency in
  `crates/fastedit/Cargo.toml`; gpui is already a dependency, so `Cargo.lock` doesn't change.
