---
task_uid: "01m46tyt7n6j8pjrppgpcrx4e8"
task: "T71"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-config test tests::layers::layers_merge_in_order: defaults \u003c user \u003c project are deep-merged key by key (a project editor.tab_size overrides the user one while the user editor.font_size survives); a missing file is an empty layer with no diagnostics"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test tests::layers::config_dir_resolution: config_dir_from gives $FASTEDIT_CONFIG_DIR when set, else $XDG_CONFIG_HOME/fastedit, else $HOME/.config/fastedit, without touching the process environment"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test tests::settings::errors_have_locations_and_keep_last_good: a TOML syntax error, font_size = \"big\" and tab_size = 0 each give one error Diagnostic with the file, the 1-based line and column of the offending token and (for the last two) the key path; after a good load, reloading the erroring file keeps the previous effective values; at startup an erroring user layer leaves defaults plus a valid project layer in effect"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test tests::settings::unknown_keys_warn: an unknown editor.colour key gives a warning naming editor.colour and its line, and the file’s other keys apply"
    required: true
    manual: false
  - id: "C5"
    text: "fe-config test tests::settings::reload_reports_only_changes: reload of an unchanged file returns None; a changed value returns a SettingsEvent with previous and new settings; only the changed layer is re-read"
    required: true
    manual: false
  - id: "C6"
    text: "fe-config test tests::settings::schema_matches_golden: settings_schema() equals crates/fe-config/settings.schema.json, which describes editor.font_family, font_size, line_height, tab_size and soft_wrap with type, default and min/max where they apply; UPDATE_GOLDEN=1 rewrites the file"
    required: true
    manual: false
  - id: "C7"
    text: "fe-config test tests::watch::hot_reload_applies_valid_and_keeps_last_good: a real ConfigWatcher over a temp-dir settings.toml delivers a batch naming the file after writing font_size = 18 in place and again after a write-to-temp-and-rename, each awaited with recv_timeout(10 s) as an upper bound; reload then gives font_size 18; after writing invalid TOML, reload gives an error diagnostic and font_size stays 18; no duration is asserted"
    required: true
    manual: false
  - id: "C8"
    text: "fe-gpui test settings_tests::apply_settings_updates_open_views: apply_settings with font_size 20, line_height 1.5 and tab_size 2 gives the next frame a 30 px row height and the model a tab size of 2; a reload event changing only font_size keeps a view’s alt-z soft-wrap toggle while one changing soft_wrap applies it; EditorTheme::default_dark equals from_settings with default settings, and the screenshots example tests pass unchanged"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-config; Cargo.lock gains no [[package]] entry other than fe-config; cargo deny check licenses bans sources passes; crates/fastedit/src/main.rs creates the ConfigWatcher only after open_window returns"
    required: true
    manual: false
---

# Add fe-config: layered TOML settings with a JSON schema, diagnostics and hot reload

# Add fe-config: layered TOML settings with a JSON schema, diagnostics and hot reload

Foundation card 3, pulled ahead of M3's remaining work. M5's goal is "Config, keymaps, themes, hot
reload". M5 is done when "TOML edits apply …; invalid config keeps the last good one and shows a
diagnostic". The plan's Architecture table says `fe-config` is "Layered TOML with `schemars` and hot
reload", GPUI-free.

This card builds the crate and wires the editor settings that exist today through to the running
app. Card 4a reuses its layer loading and watcher for the keymap.

Depends on T61, whose `notify = "8"` workspace dependency this card reuses.

## Problem

Every setting is hard-coded:

- `EditorTheme::default_dark` picks the font family (the first installed of JetBrains Mono,
  SF Mono or Menlo), and sets font size 13 and line height 1.6.
- `EditorModel` starts with `tab_size` 4 and `soft_wrap` off.

There's no settings file, no schema to give editors completions, and no way to change a value
without rebuilding.

## Change

### Workspace

- `fe-config = { path = "crates/fe-config" }` goes in `[workspace.dependencies]`.
- Add `toml = "1"`, `schemars = "1"`, `serde = { version = "1", features = ["derive"] }` and
  `serde_json = "1"` there, each only if card 1a hasn't already. All are already in `Cargo.lock`
  through gpui: toml 1.1.6, schemars 1.2.2, serde 1.0.229 and serde_json 1.0.151.
- `notify` comes from T61. `tempfile` is already a workspace dependency for tests.
- `Cargo.lock` gains no `[[package]]` except `fe-config`.
- `scripts/check-no-gpui.sh` must print `ok: fe-config`.

### fe-config (new crate)

Files: `crates/fe-config/Cargo.toml`, `src/lib.rs`, `src/paths.rs`, `src/layers.rs`,
`src/diagnostic.rs`, `src/settings.rs` and `src/watch.rs`, plus a checked-in
`crates/fe-config/settings.schema.json`.

**Paths (`paths.rs`).**

- `config_dir()` resolves in this order:
  1. `$FASTEDIT_CONFIG_DIR`
  2. `$XDG_CONFIG_HOME/fastedit`
  3. `$HOME/.config/fastedit`

  The rule lives in a pure `config_dir_from(env: impl Fn(&str) -> Option<OsString>)`, so tests
  don't mutate the environment. In edition 2024, `set_var` is unsafe.
- The user settings file is `<config_dir>/settings.toml`.
- The project settings file is `<project root>/.fastedit/settings.toml`.

**Layers (`layers.rs`).** These are generic, so card 4a can load `keymap.toml` the same way.

- `load_layer(path) -> Layer` reads and parses one TOML file. A `Layer` holds the source text, the
  parsed `toml::Table`, and diagnostics.
- A missing file is an empty layer, not an error.
- Settings deep-merge their layers table by table, in this order:
  1. built-in defaults
  2. the user file
  3. the project file
- The merged table is then deserialized and validated.

**Diagnostics (`diagnostic.rs`).** A `Diagnostic` holds a severity (error or warning), the file, the
dotted key path when known, the 1-based line and column, and a message. Line and column come from
toml's spans for:

- syntax errors
- type errors
- out-of-range values: use the value's span, for example via `toml::Spanned` or toml's spanned
  document API

Unknown keys are **warnings** that name the key path and its line, and the rest of the file
applies. This keeps settings from newer builds harmless.

**Settings (`settings.rs`).** `Settings { editor: EditorSettings }` derives serde and `JsonSchema`,
with doc comments as descriptions and defaults matching today's behaviour:

| key | type | default | valid |
|---|---|---|---|
| `editor.font_family` | string, optional | unset: first installed of JetBrains Mono, SF Mono, Menlo | non-empty |
| `editor.font_size` | float | 13.0 | 6.0–72.0 |
| `editor.line_height` | float (× font size) | 1.6 | 1.0–3.0 |
| `editor.tab_size` | integer | 4 | 1–16 |
| `editor.soft_wrap` | bool | false | |

**The store.** `SettingsStore::load(user: Option<PathBuf>, project: Option<PathBuf>)` builds the
effective `Arc<Settings>` and its diagnostics.

**Last good per layer.**

- Any error in a layer (syntax, type or range) keeps that layer's last good table and records the
  error's diagnostic. At startup the last good table is empty.
- Warnings don't reject a layer.
- `SettingsStore::reload(&changed_paths) -> Option<SettingsEvent>` re-reads only the changed
  layers. It returns `Some(SettingsEvent { settings, previous, diagnostics })` when the effective
  settings or the diagnostics changed, and `None` otherwise.

**The schema.** `settings_schema() -> serde_json::Value` comes from schemars.
`crates/fe-config/settings.schema.json` is that schema, pretty-printed. A test compares the two,
and `UPDATE_GOLDEN=1` rewrites the file, as the repo's other goldens do. Users can point taplo or
Even Better TOML at it with `#:schema`.

**The watcher (`watch.rs`).** `ConfigWatcher::new(files: Vec<PathBuf>, sink: impl FnMut(Vec<PathBuf>) + Send + 'static)`
watches each file's **parent directory** non-recursively, so atomic-rename saves are seen. It calls
the sink with the subset of `files` that changed.

- Paths are canonicalised, because `/var` is `/private/var` on macOS.
- A parent directory that doesn't exist isn't watched, and a log line says so. The watcher never
  creates directories in the user's home.

### Wiring it through

**fe-gpui**

- `EditorTheme::default_dark(cx)` becomes `EditorTheme::from_settings(&EditorSettings, cx)`.
  Keep `default_dark` as `from_settings(&Default::default(), cx)` so the screenshot goldens and
  existing callers are unchanged.
- `EditorView::apply_settings(&EditorSettings, cx)`:
  - updates the font family, size and line height
  - re-measures glyph widths for soft wrap (`set_char_widths`)
  - sets the tab size
  - redraws
- Soft wrap is applied when a view is created. On a reload, open views get the new `soft_wrap`
  only if the value changed (`event.previous` against `event.settings`), so a reload that changes
  only the font keeps each editor's `alt-z` toggle.

**fastedit `main.rs`**

- At startup it loads the store with:
  - the user file
  - the project file, rooted at the opened file's parent directory and absent with no file
- It builds the theme and the first editor from the effective settings, and logs each diagnostic
  with `log::warn!` or `log::error!`.
- The `ConfigWatcher` is created **after** the window opens, not before first paint. Its batches
  go through an async channel to a foreground task. That task calls `reload` and applies an event
  to the open editor.
- Diagnostics are logged. An in-app diagnostics surface comes with the workbench.

### Tests (new files)

- `crates/fe-config/src/tests/mod.rs`, declared from `lib.rs`, with `tests/layers.rs`,
  `tests/settings.rs` and `tests/watch.rs`. They use `tempfile` dirs only.
- `crates/fe-gpui/src/settings_tests.rs`, declared `#[cfg(test)] mod settings_tests;` from
  `crates/fe-gpui/src/lib.rs`.

The watcher test is the only one that waits on the filesystem. It waits with
`recv_timeout(Duration::from_secs(10))` as an upper bound and asserts no duration.

## Out of scope

- Themes and theme names: there are no named themes yet, so this waits for M5's theme card.
- The keymap (cards 4a and 4b).
- A settings UI, and diagnostics shown in the window.
- Per-language settings.
- Linux and Windows config locations beyond the XDG rule.

## Decided (lead defaults; the user may revisit)

- User config lives in `~/.config/fastedit/` on macOS too, not `~/Library/Application Support`.
  `$XDG_CONFIG_HOME` is honoured and `$FASTEDIT_CONFIG_DIR` overrides both. This follows the
  dotfiles convention and is the same path on every OS.
- Project settings live in `<root>/.fastedit/settings.toml`. Until the workbench exists, the root
  is the opened file's parent directory, matching T63's launch rule.
- Precedence is defaults < user < project.
- An erroring layer keeps its last good version as a whole. A bad value isn't skipped on its own,
  so a typo never silently resets a setting to its default.
- Unknown keys are warnings, not errors.
- A missing config directory isn't watched. Creating it needs a restart for hot reload.
