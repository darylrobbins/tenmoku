---
task_uid: "01m4bgw5br73vp7jv2kccyv2jt"
task: "T170"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test settings_write::preserves_the_rest (tests/settings_write.rs): on a file with a header comment, blank lines, [editor] font_size = 15.0 with a trailing comment, and [vim] enabled = false # mine, write_setting(path, \u0026[\"vim\", \"enabled\"], true) leaves the file byte-for-byte equal to the original with that one false changed to true (the trailing comment kept); write_setting(path, \u0026[\"status_bar\", \"visible\"], false) on it then leaves the previous text as an exact prefix of the file, followed by a [status_bar] table holding visible = false; a new SettingsStore::load of the result has no diagnostics, vim.enabled true, status_bar.visible false and editor.font_size 15.0"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test settings_write::creates_from_the_template: write_setting on a missing settings.toml in a missing directory creates both; the file loads with no diagnostics to Settings::default() except vim.enabled true, and every comment line of USER_SETTINGS_TEMPLATE is still in it, in order; a settings.toml that is a symlink is written through to its target and is still a symlink afterwards"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test settings_write::refuses_bad_files: for a file with a syntax error ([vim with no closing bracket) and for one holding vim = 3, write_setting(path, \u0026[\"vim\", \"enabled\"], true) returns Err, the file is byte-for-byte unchanged, and its directory holds no file other than settings.toml afterwards (no temporary file left)"
    required: true
    manual: false
  - id: "C4"
    text: "The setting: Settings::default().status_bar.visible is true; a settings.toml of [status_bar] visible = false loads with no diagnostics and visible false (asserted in tests/settings_write.rs); settings.schema.json is regenerated and its golden test passes; settings-template.toml gains a commented [status_bar] section after [vim] and tests/template.rs and tests/vim_setting.rs pass unchanged; VimSettings::enabled's doc no longer says the toggle lasts until fastedit quits"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test persist::tests::status_bar_toggle_persists: after save_toggles_to(cx, \u003ctemp\u003e/settings.toml), dispatching view.toggle_status_bar hides the bar at once and, after run_until_parked, the file loads (new SettingsStore) with status_bar.visible false; toggling again writes true; five toggles dispatched with no run_until_parked between them leave the bar hidden and, once parked, the file at visible = false; set_status_bar_visible writes nothing; a workbench where save_toggles_to was never called toggles without creating any file in its config directory"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test persist::tests::vim_toggle_persists: after save_toggles_to over a settings.toml holding [editor] tab_size = 2 and a comment, dispatching vim.toggle turns vim on and, once parked, the file loads with vim.enabled true, tab_size 2 and the comment line still present; a second vim.toggle writes false; three vim.toggle dispatches with no parking in between leave vim_enabled true and, once parked, the file at enabled = true; crates/fe-gpui is unchanged (the save hangs off fe_gpui::commands::observe_invocations)"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test persist::tests::hand_edits_apply_live: with the bar visible, writing [status_bar] visible = false to the user file and passing the SettingsStore::reload event to Workbench::reload_settings hides it, and visible = true shows it again; after view.toggle_status_bar hides the bar, a reload event that changes only editor.tab_size leaves it hidden"
    required: true
    manual: false
  - id: "C8"
    text: "fe-workbench test persist::tests::failed_write_is_reported: with save_toggles_to over a settings.toml holding a syntax error, view.toggle_status_bar still hides the bar for the session, the file is byte-for-byte unchanged once parked, and an error-level log line names the file's path (captured with a log::Log as tests::open_settings does)"
    required: true
    manual: false
  - id: "C9"
    text: "crates/fastedit/src/main.rs applies settings.status_bar.visible to the workbench before its first frame and calls fe_workbench::save_toggles_to with fe_config::user_settings_path() when that is Some; checked in the running app: toggle vim on and hide the status bar, quit, relaunch, and both are as left; then set [status_bar] visible = true by hand in settings.toml and the bar reappears without a restart"
    required: true
    manual: false
  - id: "C10"
    text: "Scope: toml_edit is the only new dependency (workspace Cargo.toml and crates/fe-config/Cargo.toml) and the deny check passes; nothing under crates/fe-gpui, crates/fe-config/src/tests or crates/fe-workbench/src/tests changes, and tests::status_bar, status_bar::vim_tests, tests::open_settings and fe-gpui's vim::tests pass unchanged"
    required: true
    manual: false
---

# Persist Toggle Vim Mode and Toggle Status Bar to the user's settings.toml, keeping its comments, with a status_bar.visible setting read at startup and on reload

# Persist Toggle Vim Mode and Toggle Status Bar to the user's settings.toml

The user asked: "Toggling vim should persist. Let's have the status bar persist too if enabled."

Today `vim.toggle` (fe-gpui's app registry, T161) flips the switch for the session only, and
`view.toggle_status_bar` only flips `Workbench::status_bar_visible`, which always starts `true`.
`docs/decisions/vim-engine.md` §10 already says the toggle "flips" the `vim.enabled` setting; this card
makes that true and does the same for the status bar, VS Code style: a toggle writes the user's
`settings.toml`, the hot reload sees the write, and a restart reads it back.

Read first: `crates/fe-config/src/settings.rs` (`SettingsStore`, layers), `crates/fe-config/src/watch.rs`
(the watcher sees temp-file-and-rename saves and follows symlinks), `crates/fastedit/src/main.rs`
(`watch_config`: vim applies on reload only when `vim.enabled` changed), `crates/fe-workbench/src/open.rs`
(`settings_path`, `create_settings`), `fe_gpui::commands::observe_invocations`.

## Change

### Writing a setting (`crates/fe-config`)

- New `src/write.rs`, re-exported from `lib.rs`:
  `pub fn write_setting(path: &Path, key: &[&str], value: bool) -> io::Result<()>` (a bool is all this
  card needs; don't generalise). It edits with `toml_edit` (add `toml_edit = "0.25"` to the workspace
  `Cargo.toml`, which is append-only shared, and to `crates/fe-config/Cargo.toml`; it is already in
  `Cargo.lock` transitively, MIT/Apache-2.0), so comments, blank lines, key order and formatting of
  everything else survive. An existing key keeps its position and its trailing comment; a missing key
  is added to its table; a missing table is added at the end of the file.
- A missing file (and its directory) is created from `USER_SETTINGS_TEMPLATE` first, then edited.
- A file that doesn't parse, or where a key on the path isn't a table (`vim = 3`), is an error and the
  file is not touched. The rest of the file is not schema-checked: that's the store's job on reload.
- The write is a temporary file in the same directory renamed over the target, so a crash never leaves
  a truncated file; a symlinked `settings.toml` is written through to its target and stays a link.

### The status bar setting (`crates/fe-config/src/settings.rs`)

- `Settings` gains `status_bar: StatusBarSettings` (`#[serde(default)]`) with `visible: bool`, default
  `true`: `[status_bar]` / `visible = false` in `settings.toml`, following `[vim]` / `enabled`.
- `VimSettings::enabled`'s doc stops saying the toggle lasts "until fastedit quits".
- Regenerate `settings.schema.json` (`UPDATE_GOLDEN=1`); `settings-template.toml` gains a commented
  `[status_bar]` section (`# visible = true` and a one-line description) after `[vim]`.

### Saving the toggles (`crates/fe-workbench`)

- New `src/persist.rs`: `pub fn save_toggles_to(cx: &mut App, path: PathBuf)`, re-exported from
  `lib.rs`. Until it's called nothing is written, so no existing test can touch a real config file.
  Once called:
  - `view.toggle_status_bar` flips the bar as now, then saves `status_bar.visible`.
  - an `observe_invocations` observer, on a successful `vim.toggle`, saves `vim.enabled` with
    `fe_gpui::vim_enabled(cx)` read after the command ran. fe-gpui does not change.
  - Writes run off the main thread, one at a time per file, and the last toggle wins: however many
    toggles queue up, the file ends at the state the app is in.
  - A failed write is logged at error level naming the file and the error; the toggle still applies
    for the session.
- `Workbench::reload_settings` also applies `status_bar.visible`, but only when it differs between
  `event.previous` and `event.settings` (as main.rs does for vim), so reloading for any other key never
  undoes a toggle. `set_status_bar_visible` stays a plain setter that writes nothing.
- Tests in a new `src/persist_tests.rs`, registered from `persist.rs` with
  `#[cfg(test)] #[path = "persist_tests.rs"] mod tests;` (as `status_bar.rs` registers
  `status_bar_vim_tests.rs`), building their own workbench in a temp dir; `src/tests/mod.rs` doesn't
  change.

### The host (`crates/fastedit/src/main.rs`)

- After `Workbench::new`, before the first frame, `set_status_bar_visible(settings.status_bar.visible)`.
- Calls `fe_workbench::save_toggles_to(cx, path)` when `fe_config::user_settings_path()` is `Some`.
- The existing reload path is unchanged apart from what `reload_settings` now does; a hand edit to
  `[status_bar] visible` applies live through it, as `[vim] enabled` already does.

## Not in this card

- A project `.fastedit/settings.toml` that sets either key still wins on reload and at startup; the
  toggle writes only the user file.
- An open `settings.toml` tab isn't reloaded when a toggle writes the file (no tab reloads from disk
  yet); saving a stale tab overwrites the toggle.
- Persisting other toggles (dock, breadcrumbs, soft wrap).

## Out of scope files

`crates/fe-gpui/**` (T162 holds `vim.rs` and `commands.rs`), `crates/fe-config/src/tests/**` (T141),
`crates/fe-workbench/src/tests/**` and `open.rs`.
