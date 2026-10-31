---
task_uid: "01m69dsvpw997kaxbtj8269jqt"
task: "T1257"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fastedit/Cargo.toml has [[bin]] name = \"tenmoku\" (package stays fastedit, no fastedit alias binary): cargo build -p fastedit builds target/\u003cprofile\u003e/tenmoku; test the_binary_is_named_tenmoku passes"
    required: true
    manual: false
  - id: "C2"
    text: "The display name is Tenmoku: app_menu_and_display_name_say_tenmoku passes (macOS app menu \"Tenmoku\" with \"Hide Tenmoku\" and \"Quit Tenmoku\"); window titles read \"\u003cfile\u003e — Tenmoku\" (pane_item::tests::titles_and_dirty_state) and the root-less title bar reads \"Tenmoku\" (tests::title_bar::no_root_reads_tenmoku)"
    required: true
    manual: false
  - id: "C3"
    text: "no_shipped_text_names_fastedit passes: menus, keymap header, settings template, settings.schema.json, WELCOME and every command_catalog() title and description say Tenmoku, apart from the .fastedit/ project dir (moved in T1258)"
    required: true
    manual: false
  - id: "C4"
    text: "Bundled themes are renamed with no alias for the old names: crates/fe-config/themes/tenmoku-{dark,light,ember}.toml exist and no fastedit-*.toml remains; titles are Tenmoku Dark, Tenmoku Light, Tenmoku Ember with ids tenmoku-dark, tenmoku-light, tenmoku-ember; find strips only \"Tenmoku \". bundled_themes_are_tenmoku, finds_by_title_or_id and tests::template_theme_names::the_template_lists_every_bundled_theme pass"
    required: true
    manual: false
  - id: "C5"
    text: "rg -n -i 'fastedit[- ](dark|light|ember)' crates prints nothing, and rg -n -i fastedit crates/fe-config/themes prints only the ~/.config/fastedit/theme.toml and $XDG_CONFIG_HOME/fastedit lines (moved in T1258)"
    required: true
    manual: false
  - id: "C6"
    text: "Other programs see Tenmoku: TERM_PROGRAM=tenmoku in fe-terminal/src/session.rs; LSP clientInfo.name \"Tenmoku\" (tests::lifecycle::initialize_advertises_m4_capabilities); User-Agent tenmoku/\u003cversion\u003e (the_request_matches_the_record); stash message \"tenmoku: before switching to \u003cbranch\u003e\" (stash_and_switch_then_pop)"
    required: true
    manual: false
  - id: "C7"
    text: "rg -n 'target/(debug|release)/fastedit|fastedit --perf|TERM_PROGRAM.{0,6}fastedit|__fastedit_' PERF.md TESTING.md docs/terminal-shell-integration.md .claude/skills/run-fastedit-linux prints nothing; driver.sh runs target/debug/tenmoku by default; the shell-integration doc has no note about pre-rename snippets"
    required: true
    manual: false
  - id: "C8"
    text: "User state paths are unchanged in this card: tests::layers::config_dir_resolution and state_dir_resolution pass unmodified"
    required: true
    manual: false
---

# Rename the app to Tenmoku where users see it: a tenmoku binary, app menu, window titles, prompts, bundled theme names, ids and files, and docs

## Goal

The editor is renamed **Tenmoku** (the user owns tenmoku.dev and tenmoku.app). This card changes
what users see, the installed binary's name and the bundled themes' titles, ids and files. Where
user state lives (`~/.config/fastedit`, `~/.local/state/fastedit`, `FASTEDIT_CONFIG_DIR`/
`FASTEDIT_STATE_DIR`, the keychain service), the project dir `.fastedit/` and the extension
engine key `fastedit` are unchanged here: the follow-up card (T1258) moves them.

**No backwards compatibility** (user's steer, 2026-10-31: they are the only one who has run it).
Nothing resolves an old "Fastedit …" title or `fastedit-*` theme id, and no doc mentions the old
name for people with pre-rename copies.

Out of scope and kept as-is: crate and package names (`fastedit`, `fe-*`), module paths, Rust
identifiers (`EditorPalette::fastedit_dark()`/`fastedit_light()`, the `FASTEDIT_DARK`/
`FASTEDIT_LIGHT` statics: code, not user-facing) and log targets (`fastedit::…`, `fe_…`), repo
paths, Regatta and CI config, docs/specs and docs/decisions (historical records), and these
stable internal ids: pane item `KIND`s (`fastedit.editor`, …, persisted in saved layouts), chat
agent ids (`fastedit.chat`, `fastedit.inline_edit`), the WIT package `fastedit:editor` and
capability names, temp-file names (`.fastedit-save`, `.fastedit-<what>-…`),
`FASTEDIT_LOGIN_PATH_FILE` (parent-to-login-shell only) and the dev/CI variables
(`FASTEDIT_MIN_FREE_GIB`, `FASTEDIT_AFFECTED_*`, `FASTEDIT_PRUNE_TARGET`, `FASTEDIT_HOSTS`,
`FASTEDIT_TEST_BASE`). Test-only kiln ids such as `Config::new("dev.fastedit.test", "fastedit")`
are left alone.

## Facts found

- No `.app` bundle, Info.plist or .desktop file exists. The app runs unbundled;
  `crates/fastedit/src/app_icon.rs` sets the Dock icon at startup. Unbundled, macOS shows the
  **executable name** as the bold app-menu title and Dock label, so after this card they read
  `tenmoku` (lower case). A capitalised "Tenmoku" there needs a bundle: out of scope, note it in
  the hand-in.
- The display name kiln uses (`Config::new(app.reverse_dns, app.display)` in
  `crates/fastedit/src/config.rs`) comes from `fe_config::paths::app_id()`
  (`AppId::new("dev.fastedit.fastedit", "fastedit", "fastedit")`). Only its third argument
  (display) changes here; `reverse_dns` and `short` change in T1258, because they key the
  config/state dirs, the env overrides, the keychain entry, the project dir and the engine key.
- The app menu is fastedit's own `crates/fe-keymap/menus/fastedit.toml` (`name = "fastedit"`,
  "Hide fastedit", "Quit fastedit"); `fe-gpui/src/commands.rs` `APP_NAME` titles app commands.
- Nothing spawns the binary by name in tests (`CARGO_BIN_EXE_*` unused). By name it is invoked in
  PERF.md, TESTING.md, `crates/fastedit/src/perf.rs` docs, and
  `.claude/skills/run-fastedit-linux/driver.sh` (`BIN=…/target/debug/fastedit`, which CI's
  launch job runs).
- Bundled themes: `fe-config/src/themes.rs` `BUNDLED` lists `bundled!("Fastedit Dark",
  "fastedit-dark", Dark)` etc.; the macro `include_str!`s `../themes/<id>.toml`, so id and file
  name move together. `find` strips a "Fastedit " prefix, which is what makes `find("dark")`
  return the bundled dark theme. `fe-config/src/theme.rs` builds the base palettes from
  `fastedit-dark.toml`/`fastedit-light.toml` (`bundled("<file>", include_str!(…))`).
  `fe-workbench/src/theme_picker.rs` `showing()` falls back to the ids `fastedit-dark`/
  `fastedit-light` when no theme is chosen. The theme files' `name` fields are lower-case
  "fastedit Dark/Light/Ember". The settings schema has no default theme id; the
  `theme.name` doc example ("Dark Modern" or "Fastedit Ember") is in `settings.rs`, so in
  `settings.schema.json`, and the template lists the bundled titles.
- Every `fastedit-{dark,light,ember}` reference outside docs/specs and docs/decisions
  (`rg -n 'fastedit-(dark|light|ember)'`): `fe-config/src/{themes,theme}.rs`,
  `fe-workbench/src/theme_picker.rs`, `fe-workbench/src/theme_picker_tests.rs`,
  `fastedit/src/theme_name_tests.rs`, `fe-gpui/src/element/{syntax_underline_color_tests,
  jump_style_tests}.rs`, and `fe-config/tests/{bundled_themes, ember_theme, jump_label_contrast,
  semantic_theme_keys, format_specifier_theme_key, vscode_2026_dark_theme,
  vscode_2026_light_theme, sticky_scroll_colors, editor_palette, vim_mode_colors,
  whitespace_rulers_settings, indent_guide_colors, change_colors_muted}.rs`.

## Changes

1. **Binary.** In `crates/fastedit/Cargo.toml` add `[[bin]] name = "tenmoku"`,
   `path = "src/main.rs"`, and set `description` to name Tenmoku. Package name stays `fastedit`, so
   `cargo run -p fastedit` still works and `cargo install --path crates/fastedit` installs
   `tenmoku`. **No `fastedit` alias binary.**
2. **Display name** "Tenmoku" (capitalised): `fe_config::paths::app_id()` display argument only;
   `APP_NAME` in `fe-gpui/src/commands.rs`; the menus TOML app menu `name = "Tenmoku"`,
   "Hide Tenmoku", "Quit Tenmoku"; `WindowSpec::main("Tenmoku")` (3 sites in
   `crates/fastedit/src/main.rs`); `Workspace::…title("Tenmoku")` and `WELCOME` in
   `fe-workbench/src/workbench.rs` (the welcome text says `tenmoku <path>`, not `cargo run`);
   `pane.rs` no-item title; `title_bar.rs` `NO_ROOT`; every `window_title` `"… — fastedit"`
   (fe-gpui `pane_item.rs`, `image_viewer/pane_item.rs`, `file_viewer/pane_item.rs`,
   `diff_view.rs`; fe-workbench `markdown_preview.rs`, `shortcuts_editor.rs`,
   `settings_editor.rs`) becomes `"… — Tenmoku"`. Where a crate already depends on fe-config,
   prefer `fe_config::paths::app_id().display` (or one `pub const APP_NAME` in fe-config) over a
   new literal.
3. **Prose that names the app** says Tenmoku: trust prompt (`fe-workbench/src/trust.rs`),
   `workspace_edit.rs` errors (3), `lsp.rs` toast, `fe-lsp` `registration.rs` and `launcher.rs`
   messages, `fe-vim` `vimrc.rs` (`Select mode isn't in Tenmoku`) and the `folds.rs` command
   description, chat system prompts (`fe-chat/src/request.rs`, `inline.rs`), the keymap file
   header (`fe-keymap/src/keymap_edit.rs` `KEYMAP_HEADER`), the backup file header
   (`fe-buffer/src/backup.rs`), and the settings docs that become schema descriptions
   (`fe-config/src/settings.rs` doc comments, including the `theme.name` example
   "Tenmoku Ember"; regenerate `settings.schema.json` with `UPDATE_GOLDEN=1`) and
   `settings-template.toml` (its `.fastedit/settings.toml` mention stays until T1258).
4. **Identifiers other programs see:** `TERM_PROGRAM=tenmoku` (`fe-terminal/src/session.rs`), LSP
   `clientInfo.name = "Tenmoku"` (`fe-lsp/src/server.rs`), the Anthropic User-Agent
   `tenmoku/<version>` (`fe-chat-anthropic/src/wire.rs`), the git stash message
   `tenmoku: before switching to <branch>` (`fe-git/src/branch.rs`; nothing looks stashes up by
   that prefix).
5. **Bundled themes: titles, ids and files.**
   - `git mv` `crates/fe-config/themes/fastedit-{dark,light,ember}.toml` to
     `tenmoku-{dark,light,ember}.toml`; their `name` fields become "Tenmoku Dark",
     "Tenmoku Light", "Tenmoku Ember" and their header comments say Tenmoku (the
     `~/.config/fastedit/theme.toml` lines in `tenmoku-ember.toml` stay for T1258).
   - `themes.rs` `BUNDLED`: `bundled!("Tenmoku Dark", "tenmoku-dark", Dark)`, likewise Light and
     Ember. `find` strips only a "Tenmoku " prefix (doc comment says so). No alias: "Fastedit
     Ember" and `fastedit-ember` no longer resolve.
   - `theme.rs` `FASTEDIT_DARK`/`FASTEDIT_LIGHT` read `tenmoku-dark.toml`/`tenmoku-light.toml`
     (file-name argument and `include_str!` path); the Rust names stay.
   - `theme_picker.rs` `showing()` fallback ids `tenmoku-dark`/`tenmoku-light`.
   - The other bundled theme files' prose that names the app or its themes
     (`catppuccin-*.toml`, `vscode-2026-{dark,light}.toml`, `vscode-dark-modern.toml`:
     "fastedit's one-colour-per-highlight-name model", "Every name fastedit Dark colours …",
     "fastedit Light's") says Tenmoku; their `~/.config/fastedit/theme.toml` lines stay for T1258.
   - Template and schema docs list "Tenmoku Dark, Tenmoku Light or Tenmoku Ember".
   - Every test in the Facts list switches to the new ids, titles and file paths
     (`include_str!("../themes/tenmoku-…")`, `"builtin/tenmoku-…"`, `find("Tenmoku Light")`,
     `ember.id == "tenmoku-ember"`); the theme-name labels in those files' assert tables say
     Tenmoku. `bundled_themes.rs::finds_by_title_or_id` drops its `Fastedit Ember`/
     `FASTEDIT-EMBER` lookups for `Tenmoku Ember`/`TENMOKU-EMBER`. The remaining "fastedit
     Dark/Light" labels and comments say Tenmoku too (`fe-gpui/src/element/jump_tests.rs` labels,
     `fe-gpui/src/element/sticky_scroll_tests.rs` comment, `fe-gpui/src/lib.rs` two doc lines),
     so `rg -n -i 'fastedit[- ](dark|light|ember)' crates` comes up empty.
6. **Docs and tooling:** `docs/terminal-shell-integration.md` (TERM_PROGRAM check `tenmoku`,
   helper functions `__tenmoku_*`; no note about pre-rename snippets); PERF.md and TESTING.md
   (`target/release/tenmoku --perf`, `tenmoku --perf`; prose naming the product); `NOTICE` first
   line; `.claude/skills/run-fastedit-linux/driver.sh` default `BIN` `target/debug/tenmoku` and its
   SKILL.md (skill name and run dir stay).
7. **Tests** whose expectations name the app are updated, renamed where the name says fastedit
   (`no_root_reads_fastedit` → `no_root_reads_tenmoku`). Tests passing `"fastedit"` as an engine
   key or a `Config::new` test id are left alone here (T1258 moves the engine key).

New tests in a new file `crates/fastedit/tests/app_name.rs`:
- `the_binary_is_named_tenmoku`: `env!("CARGO_BIN_EXE_tenmoku")` exists (compile-time) and its
  file stem is `tenmoku`.
- `app_menu_and_display_name_say_tenmoku`: `fe_config::paths::app_id().display == "Tenmoku"`;
  the macOS menus' first menu is named "Tenmoku" with items "Hide Tenmoku" and "Quit Tenmoku".
- `no_shipped_text_names_fastedit`: case-insensitive, none of these contain `fastedit`, except
  the `.fastedit/` project-dir mention (T1258 removes this exception): the menus TOML (all
  platforms), the keymap header, the settings template, `settings.schema.json`, the welcome text,
  and every command title and description in `fe_workbench::command_catalog()`.
- `bundled_themes_are_tenmoku`: the three are titled "Tenmoku Dark", "Tenmoku Light",
  "Tenmoku Ember" with ids `tenmoku-dark`, `tenmoku-light`, `tenmoku-ember`; `find("Tenmoku
  Ember")`, `find("Ember")` and `find("tenmoku-ember")` all return the Ember one.

Hand-in note for the user: their own `settings.toml` may name `Fastedit Ember` or
`builtin/fastedit-*`; it now falls back to the default theme until they edit it.

## Conflicts

`crates/fastedit/Cargo.toml` is in T1254 (landed) and T1235 (blocked, also
`crates/fastedit/tests/**`, so `app_name.rs`: if T1235 lands first, declare the new file the way
it requires); `TESTING.md` is in T1256 (active), T1234 and T1235 (blocked);
`fe-config/src/settings.rs`, `settings.schema.json`, `settings-template.toml` and
`crates/fastedit/src/main.rs` are in T659 and T1168 (blocked); `fe-gpui/src/diff_view.rs`,
`fe-config/src/theme.rs` and `fe-config/themes/*.toml` are in T1252 (blocked: the theme-file
rename is the biggest overlap, so whichever lands second rebases onto the renamed files).
`rgt task conflicts` also flags T1210 (blocked) through its `crates/fe-workbench/src/lsp_colors_*`
glob; no real file is shared. Every other overlap is a few lines; rebase rather than wait where
the daemon allows.

## Implementation notes

- The display name lives once, as `fe_config::APP_NAME` (`paths.rs`, re-exported from
  `fe-config/src/lib.rs`); `app_id()`, fe-gpui's `APP_NAME`, `WindowSpec::main`, the workspace
  title, the pane's no-item title, the title bar's `NO_ROOT` and every `window_title` use it.
- `CommandCatalog` only checks ids, so `no_shipped_text_names_fastedit` reads titles and
  descriptions from an initialised test app: kiln's typed commands, the app registry
  (`app_command_list`) and the workbench's registries plus the editor's, for which
  `fe_workbench::command_list` became `pub` (`fe-workbench/src/lib.rs`, outside the listed scope).
- The keymap header no longer names `keymaps/fastedit.toml` (the built-in keymap's file name),
  so it carries no "fastedit".
- Unbundled, macOS still shows the executable name, lower-case `tenmoku`, as the bold app-menu
  title and Dock label; a capitalised "Tenmoku" there needs an `.app` bundle.
- A user `settings.toml` naming `Fastedit Ember` or `builtin/fastedit-*` now falls back to the
  default theme until edited.
