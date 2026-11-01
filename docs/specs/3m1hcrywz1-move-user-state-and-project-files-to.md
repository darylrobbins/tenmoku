---
task_uid: "01m69dswwc4s5h8c3m1hcrywz1"
task: "T1258"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe_config::paths::app_id() is (\"dev.tenmoku.tenmoku\", \"tenmoku\", \"Tenmoku\"). Config resolves TENMOKU_CONFIG_DIR, then $XDG_CONFIG_HOME/tenmoku, then ~/.config/tenmoku, and state likewise with TENMOKU_STATE_DIR and ~/.local/state/tenmoku: app_id_is_tenmoku, tests::layers::config_dir_resolution and state_dir_resolution pass"
    required: true
    manual: false
  - id: "C2"
    text: "No FASTEDIT_ fallback: fastedit_dir_variables_are_ignored passes, and paths.rs passes env straight to AppPaths::resolve (no wrapper mapping one variable to another)"
    required: true
    manual: false
  - id: "C3"
    text: "The project dir and engine key follow short: project settings at \u003croot\u003e/.tenmoku/settings.toml and workspace extensions at \u003croot\u003e/.tenmoku/extensions (app_id_is_tenmoku); every test manifest says [engine] tenmoku; crates/fastedit tests/extension_*.rs and the fe-workbench extensions_picker tests pass; config.rs still passes \u0026app_id().short and no PROJECT_DIR or EXTENSION_APP constant exists"
    required: true
    manual: false
  - id: "C4"
    text: "The chat key is under keychain service dev.tenmoku.tenmoku with no fallback to dev.fastedit.fastedit: chat_panel::chat_panel_tests::set_key::the_set_api_key_button_stores_the_key_and_the_chat_uses_it and chat_panel::chat_panel_tests::the_key_comes_from_the_environment_then_the_keychain pass with the new service"
    required: true
    manual: false
  - id: "C5"
    text: "No migration: crates/fe-config has no migrate module and main.rs adds no startup step; nothing reads the old dirs or the old keychain entry"
    required: true
    manual: false
  - id: "C6"
    text: "The 3 WindowSpec::main sites in crates/fastedit/src/main.rs set .app_id(app_id().reverse_dns) for Wayland and X11"
    required: true
    manual: false
  - id: "C7"
    text: "no_shipped_text_names_fastedit passes with its .fastedit/ exception removed"
    required: true
    manual: false
  - id: "C8"
    text: "Every test and doc isolates with TENMOKU_CONFIG_DIR/TENMOKU_STATE_DIR and names .tenmoku/: at the repo root, rg -n --hidden -g '!.git' -g '!docs/specs/**' -g '!docs/decisions/**' 'FASTEDIT_(CONFIG|STATE)_DIR|\\.fastedit(/|\"\\)|`)|(\\.config|state|_HOME)/fastedit|dev\\.fastedit\\.fastedit|\\[engine\\](\\\\n| )fastedit|^fastedit = ' prints nothing"
    required: true
    manual: false
---

# Move user state and project files to Tenmoku's names: ~/.config/tenmoku, ~/.local/state/tenmoku, TENMOKU_*_DIR, keychain dev.tenmoku.tenmoku, .tenmoku/ and [engine] tenmoku, with no migration

## Goal

Following the Tenmoku rename of what users see (T1257), everything else users touch moves to
Tenmoku's names: the config and state directories, their `TENMOKU_CONFIG_DIR`/
`TENMOKU_STATE_DIR` overrides, the keychain entry, the project directory `<root>/.tenmoku/` and
the extension manifests' engine key `[engine] tenmoku`.

**No migration and no backwards compatibility** (user's steer, 2026-10-31: they are the only
one who has run it). Nothing reads `~/.config/fastedit`, `~/.local/state/fastedit`,
`FASTEDIT_CONFIG_DIR`, `FASTEDIT_STATE_DIR`, the `dev.fastedit.fastedit` keychain entry, a
`.fastedit/` project dir or an `[engine] fastedit` manifest after this card.

Kept as-is (as in T1257): crate/package names, module paths, Rust identifiers, log targets,
repo paths, Regatta/CI config, docs/specs and docs/decisions, the WIT package `fastedit:editor`
and capability names, pane item `KIND`s, chat agent ids, temp-file names (`.fastedit-save`,
`.fastedit-<what>-…`), `FASTEDIT_LOGIN_PATH_FILE`, the dev/CI variables (`FASTEDIT_MIN_FREE_GIB`,
`FASTEDIT_AFFECTED_*`, `FASTEDIT_PRUNE_TARGET`, `FASTEDIT_HOSTS`, `FASTEDIT_TEST_BASE`), and
test-only kiln ids such as `Config::new("dev.fastedit.test", "fastedit")`.

## Where state lives today (verified)

Every user path goes through `crates/fe-config/src/paths.rs`:
`app_id() = AppId::new("dev.fastedit.fastedit", "fastedit", <display>)`, resolved by kiln-core's
`AppPaths::resolve` with **`PathStyle::Xdg` on every platform**:
- config: `$FASTEDIT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/fastedit`, else `~/.config/fastedit`
  (settings.toml, keymap.toml, theme.toml, vimrc, themes/)
- state: `$FASTEDIT_STATE_DIR`, else `$XDG_STATE_HOME/fastedit`, else `~/.local/state/fastedit`
  (trust.toml, layouts/, backups/, recent.toml, extensions/, the extension consent file, chats/)
- On macOS nothing lives under `~/Library/Application Support`, `~/Library/Caches` or
  `~/Library/Logs` (logging goes to stderr only).
- `fe_keymap::user_keymap_path`, `fe_config::recent`, `trust_path`, `fe-workbench` `chat_panel`
  state and hot-exit backups all derive from these two dirs.
- The `FASTEDIT_` prefix of the env overrides is derived from `short` (`AppId::env_var`).
- The chat's Anthropic key is in the platform keychain under service `app_id().reverse_dns`
  (`fe-workbench/src/chat_panel.rs` `key_service()`), account `chat.anthropic`.
- `short` is also the project dir and the extension engine key, with no fastedit code naming
  them: kiln-core's `AppPaths::project_dir` is `<root>/.<short>` (so `project_settings_path` is
  `<root>/.<short>/settings.toml`), and `crates/fastedit/src/config.rs` passes
  `&app_id().short` to `.extensions(dir, app, api)`, which kiln uses as the manifests' `[engine]`
  key and for workspace extensions at `<root>/.<app>/extensions`.
- There is no `.fastedit/` dir at the repo root, and no tracked file under any `.fastedit/`.

## Decisions

- `app_id()` becomes `AppId::new("dev.tenmoku.tenmoku", "tenmoku", "Tenmoku")` (the existing
  `dev.<name>.<name>` scheme, on the user's tenmoku.dev domain). That one line moves, through
  kiln, the dirs (`~/.config/tenmoku`, `~/.local/state/tenmoku` and their `XDG_*_HOME` forms),
  the overrides (`TENMOKU_CONFIG_DIR`, `TENMOKU_STATE_DIR`), the keychain service
  (`dev.tenmoku.tenmoku`), the project dir (`<root>/.tenmoku/`) and the engine key (`tenmoku`).
  No env-mapping wrapper, no `PROJECT_DIR`/`EXTENSION_APP` constants, no kiln change, and
  `chat_panel.rs` and `config.rs` code are unchanged (only `config.rs`'s doc line naming
  `[engine] fastedit = "1.0"` changes).
- No migration module, no startup step, no keychain fallback.
- On Linux, the windows get `WindowSpec::…app_id(app_id().reverse_dns)` (Wayland app_id and X11
  WM_CLASS), in the 3 `WindowSpec::main` sites in `crates/fastedit/src/main.rs`. No .desktop file
  exists, so none is added.
- Hand-in note for the user (no code): to keep their state, by hand
  `mv ~/.config/fastedit ~/.config/tenmoku`, `mv ~/.local/state/fastedit ~/.local/state/tenmoku`,
  re-enter the chat key (Set API Key), rename any repo's `.fastedit/` to `.tenmoku/`, and change
  their own extensions' `[engine] fastedit` to `tenmoku`.

## Changes

1. **paths.rs:** the `app_id()` line; module and function docs name `tenmoku`,
   `TENMOKU_CONFIG_DIR`/`TENMOKU_STATE_DIR` and `<root>/.tenmoku/settings.toml`.
2. **Test isolation** (`rg -n 'FASTEDIT_(CONFIG|STATE)_DIR'`, which finds nothing in kiln,
   scripts/, .claude/skills or .github). Each must switch to `TENMOKU_*`, or that test reads or
   writes the user's real `~/.config/tenmoku`:
   - `fe-config/src/tests/layers.rs` (`config_dir_resolution`: variable names, expected
     `/xdg/tenmoku` and `/home/me/.config/tenmoku`)
   - `fe-config/tests/workspace_trust.rs` (`state_dir_resolution`, likewise)
   - `fe-config/tests/settings_edit.rs` (`set_var`)
   - `fe-workbench/src/settings_editor_choices_tests.rs` (`.env(…)` on the own-process re-run,
     and its doc comments)
   - `fe-workbench/src/settings_editor_controls_tests.rs` (`.env(…)`, the `set_var` in the
     two-config-dirs test, doc comments)
   - `fe-workbench/src/settings_editor.rs` (the `#[cfg(test)]` guard's message)
   - `fe-workbench/tests/{find_replace_interrupted, git_log_restore_first_list,
     hot_exit_rootless, palette_ghost_text}.rs` (the `remove_var` lists)
   - `TESTING.md` (the `FASTEDIT_CONFIG_DIR=/tmp/fe-lsp-check cargo run` example)
3. **Project dir `.fastedit/` → `.tenmoku/`** (`rg -n '\.fastedit(/|"\)|`)'`):
   - doc comments: `fe-config/src/{settings_model,settings_edit}.rs`,
     `fe-workbench/src/{workbench_root,lsp,settings_editor,lsp_trusted_settings_tests}.rs`,
     `fastedit/src/workspace_extensions.rs`
   - `fe-config/settings-template.toml` line 3
   - tests that make the dir: `fe-config/tests/settings_edit.rs`,
     `fe-workbench/src/{settings_editor_tests, settings_editor_caret_tests,
     settings_editor_choices_tests, settings_editor_controls_tests,
     settings_editor_controls_blur_tests, extensions_picker_workspace_tests}.rs`,
     `fe-workbench/src/tests/settings_caret.rs`, `fastedit/tests/extension_workspace.rs`
   - `crates/fastedit/tests/app_name.rs`: `no_shipped_text_names_fastedit` drops its
     `.fastedit/` exception.
4. **Engine key `[engine] fastedit` → `tenmoku`** in every manifest a test writes:
   `fastedit/src/theme.rs` (test), `fastedit/tests/extension_{workspace, servers, snippets,
   disabled, consent, grammar_swap, grammars}.rs`, `fe-workbench/src/{settings_editor_choices_tests,
   extensions_picker_tests, extensions_picker_disable_tests, extensions_picker_workspace_tests,
   theme_picker_tests}.rs`. The fe-workbench tests that pass `"fastedit"` as kiln's extension
   app (`.extensions(dir, "fastedit", api)`) pass `&fe_config::paths::app_id().short` instead,
   so they model the real app.
5. **Keychain:** the chat tests seeding `"dev.fastedit.fastedit"`
   (`chat_key_tests.rs` `SERVICE`, `chat_panel_tests.rs`, `chat_rail_tests.rs`) use
   `"dev.tenmoku.tenmoku"`.
6. **User-dir docs say `tenmoku`:** `fastedit/src/vim_remaps.rs` module doc, and the "copy it to
   ~/.config/fastedit/theme.toml" lines (and `$XDG_CONFIG_HOME/fastedit`) in
   `fe-config/themes/{catppuccin-frappe, catppuccin-latte, catppuccin-macchiato, catppuccin-mocha,
   tenmoku-ember, vscode-2026-dark, vscode-2026-light, vscode-dark-modern}.toml`. Tidy-ups:
   `fe-config/tests/settings_write.rs` and `fe-workbench/src/tests/open_settings.rs` name their
   temp config dir `tenmoku`.
7. **main.rs:** the 3 `WindowSpec::main` sites add `.app_id(app_id().reverse_dns)`.

## Tests

Updated in place: `tests::layers::config_dir_resolution`, `state_dir_resolution`, the
extension, settings-editor and chat-key tests above (their expectations now prove the new
project dir, engine key and keychain service: a manifest still saying `fastedit`, or a dir still
named `.fastedit`, makes them fail).

New file `crates/fe-config/tests/tenmoku_paths.rs` (public API only):
- `app_id_is_tenmoku`: `app_id()` is `("dev.tenmoku.tenmoku", "tenmoku", "Tenmoku")`;
  `project_settings_path(root)` is `root/.tenmoku/settings.toml` and
  `app_paths().project_extensions_dir(root)` is `root/.tenmoku/extensions`.
- `fastedit_dir_variables_are_ignored`: with `FASTEDIT_CONFIG_DIR`/`FASTEDIT_STATE_DIR` and
  `HOME=/h` only, `config_dir_from` is `/h/.config/tenmoku` and `state_dir_from` is
  `/h/.local/state/tenmoku`.

## Conflicts

Depends on T1257, which also edits `paths.rs`, `main.rs` (the same 3 `WindowSpec::main` lines),
`settings-template.toml`, `theme_picker_tests.rs`, `app_name.rs` (created there) and the theme
files (it renames `fastedit-ember.toml` to `tenmoku-ember.toml`). Others: `crates/fastedit/src/main.rs`
and `settings-template.toml` (T659, T1168 blocked); `crates/fastedit/tests/**` and
`crates/fe-workbench/tests/**` (T1235 blocked); `fe-config/themes/*.toml` (T1252 blocked);
`TESTING.md` (T1256 active, T1234 and T1235 blocked). `rgt task conflicts` also flags T1210
(blocked) through its `crates/fe-workbench/src/lsp_colors_*` glob; no real file is shared.
