---
task_uid: "01m4f8qms6mzpa6fnssyzva41m"
task: "T244"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test theme_store::selects_and_switches: ThemeStore::load over a tempdir holding a theme named \"Paper\", with name \"paper\", makes theme() Paper; select(\"fastedit dark\") returns a ThemeEvent with previous Paper and theme fastedit Dark; selecting the current name again returns None"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test theme_store::unknown_name_falls_back: load or select with \"Nope\" makes theme() fastedit Dark, with exactly one Warning diagnostic whose message names \"Nope\""
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test theme_store::reload_reads_changed_files: rewriting the selected file's editor.background and calling reload with its path returns an event with the new colour; reload with a path not in paths() returns None; a theme file written after load is in paths() once select names it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test theme_store::broken_selected_file_keeps_last_good_theme: a TOML syntax error saved into the selected file leaves theme() unchanged after reload and reports an Error diagnostic for that file; fixing the file reloads the fixed theme"
    required: true
    manual: false
  - id: "C5"
    text: "fe-ui test active_theme::defaults_and_notifies: ui_theme(cx) equals UiTheme::default_dark() before any set_ui_theme; afterwards it returns the theme that was set, and an observe_global::\u003cActiveUiTheme\u003e observer has run"
    required: true
    manual: false
---

# Add a ThemeStore to fe-config that selects and hot-reloads themes, and an fe-ui global for the active UiTheme

## Problem

T238 (e1ca57e) added the GPUI-free theme model in `crates/fe-config/src/theme.rs`: `Theme`, `ThemeSet`,
`load_themes(dir)`, the bundled fastedit Dark in `crates/fe-config/themes/fastedit-dark.toml`, and
`paths::themes_dir()`. Nothing uses it yet. Its spec left the app wiring for later: "a `theme = "<name>"`
setting", "converting to `UiTheme`/`EditorTheme` and applying the theme", and "hot reload of the themes
directory". The user asked for that wiring to be filed once T238 landed (plan approved 2026-10-08).

Two pieces of it touch no busy file, so they go first:

- Nothing holds the selected theme across reloads the way `SettingsStore` holds settings
  (`crates/fe-config/src/settings.rs`, `SettingsStore::reload`), so a host can't hot-reload a theme.
- The workbench chrome copies `UiTheme::default_dark()` (`crates/fe-ui/src/theme.rs`) into every panel at
  construction (`crates/fe-workbench/src/workbench.rs:186`), so no host can supply other colours.
  `crates/fe-ui/Cargo.toml` says fe-ui is "GPUI only: no workspace crate, so any host can use these". So the
  seam for host-provided chrome colours is a GPUI global in fe-ui that holds a `UiTheme`, not a
  `fe_config::Theme`. A coming fe-shell (T242) will need theming from its host. This card doesn't design
  fe-shell; it only keeps the seam free of workbench ownership.

## Change

**`ThemeStore`**: a new file, `crates/fe-config/src/theme_store.rs`, exported from
`crates/fe-config/src/lib.rs`. No GPUI. It works like `SettingsStore`:

- `ThemeStore::load(dir: Option<PathBuf>, name: &str) -> ThemeStore` runs `load_themes(dir)` and selects
  `name` with `ThemeSet::get`, which ignores case.
- `theme(&self) -> &Arc<Theme>`, `diagnostics(&self) -> &[Diagnostic]` (the last read's diagnostics plus any
  selection warning), and `paths(&self) -> Vec<PathBuf>`: every `*.toml` file the last read found, sorted,
  for a `ConfigWatcher`.
- `reload(&mut self, changed: &[PathBuf]) -> Option<ThemeEvent>` re-reads the directory if any path in
  `changed` is one of `paths()`. It returns `None` if none is, or if neither the theme nor the diagnostics
  changed.
- `select(&mut self, name: &str) -> Option<ThemeEvent>` re-reads the directory, so a theme file added since
  the last read can be chosen, then selects `name`. It returns `None` if nothing changed.
- `ThemeEvent { theme: Arc<Theme>, previous: Arc<Theme>, diagnostics: Vec<Diagnostic> }`.
- **Unknown name.** If `load` or `select` names a theme that isn't listed, the store uses fastedit Dark. It
  adds a `Severity::Warning` diagnostic whose `file` is the themes directory (an empty path when `dir` is
  `None`), with `line` and `column` 1. The message names the missing theme and says fastedit Dark is in use.
- **Last good theme.** If `reload` leaves the selected name unlisted, the store keeps the current theme.
  That happens when the selected file now has a TOML syntax error or has been deleted. The read's
  diagnostics are still reported, so a half-saved edit never flashes fastedit Dark. This matches
  `SettingsStore` keeping a layer's last good table.

**The fe-ui global**: in `crates/fe-ui/src/theme.rs`, exported from `crates/fe-ui/src/lib.rs`. fe-ui
gains no workspace dependency.

- `pub struct ActiveUiTheme(pub UiTheme)` implements `gpui::Global`.
- `pub fn set_ui_theme(cx: &mut App, theme: UiTheme)` sets the global.
- `pub fn ui_theme(cx: &App) -> UiTheme` returns the global, or `UiTheme::default_dark()` if none is set.
- Components follow changes with `cx.observe_global::<ActiveUiTheme>`. A host sets the global. fe-ui never
  reads settings or theme files.
- Update the doc comment on `UiTheme` ("Hard-coded until themes land (M5)") to say what sets it now.

Not in this card: the `theme` setting, conversion to `UiTheme`/`EditorTheme`, the host wiring, and the
workbench following the global. Later cards do those.

## Tests

- `crates/fe-config/tests/theme_store.rs`: a new file using `tempfile` directories.
- `crates/fe-ui/src/tests/active_theme.rs`: a new file, with one `mod active_theme;` line added to
  `crates/fe-ui/src/tests/mod.rs`. It needs `gpui` with `features = ["test-support"]` as a dev-dependency
  in `crates/fe-ui/Cargo.toml`, as `crates/fe-gpui/Cargo.toml` has it.

## Implementation notes

- **`select` with the selected name behaves as a reload.** If `select` names the theme already selected
  (ignoring case) and it is now unlisted because its file is broken or gone, the store keeps the last
  good theme rather than falling back to fastedit Dark. A host is likely to call `select` again with the
  same name whenever the settings reload, and falling back there would flash fastedit Dark on a
  half-saved theme, which the "Last good theme" rule exists to prevent. Selecting a different unlisted
  name, or selecting under `load`, falls back as specified.
- **A kept theme gets a warning.** When a reload keeps the last good theme, the store adds a
  `Severity::Warning` (at the themes directory, line 1, column 1) naming the theme, beside the read's
  own diagnostics. Without it a deleted theme file would leave no diagnostic at all.
- **A kept theme's file stays in `paths()`.** While the last good theme is kept, files gone since the
  previous read stay in `paths()`. An editor that saves by moving the old file away and writing a new one
  produces a delete and then a create; without this the create would be ignored until the next `select`.
- **The fallback is the bundled fastedit Dark** (`Theme::fastedit_dark()`), even if a user file overrides
  a theme of that name.
- **Known limits.** `paths()` is listed just after `load_themes` reads the directory, as `theme.rs` (out
  of scope) doesn't report the files it read, so a file created or deleted between the two may be in one
  and not the other; the next read reconciles them. If two files name the same theme and the later one
  breaks, the earlier one's theme is listed under that name and is used, as `load_themes` resolves it.
