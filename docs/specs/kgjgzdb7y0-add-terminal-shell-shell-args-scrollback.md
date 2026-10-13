---
task_uid: "01m4s6vyme3yr030kgjgzdb7y0"
task: "T62"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config: Settings has a terminal table (shell \"\", shell_args [], scrollback 10000 in 0..=100000, font_family and font_size unset, font_size in 6..=72); settings.schema.json and settings-template.toml list every key, and tests/template.rs checks them like the editor's"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config tests/terminal_settings.rs: a project file's terminal.shell and terminal.shell_args are dropped with a PROJECT_TERMINAL_SHELL warning at each key while its other terminal keys apply; out-of-range scrollback and font_size are diagnostics that keep the last good value"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench terminal_settings_tests.rs: a session started with terminal.shell = /bin/sh and shell_args running printf shows its output with no set_shell override; empty shell still starts the login shell path (TerminalOptions.shell None)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench terminal_settings_tests.rs: a settings reload changing terminal.font_size or font_family restyles the running session without restarting it, and one changing shell keeps the running session while the next session started runs the new shell"
    required: true
    manual: false
  - id: "C5"
    text: "fastedit's new_workbench passes settings.terminal to the workbench at window creation; cargo test -p fe-config -p fe-workbench and cargo clippy --workspace --all-targets -- -D warnings pass; no keymap or menu file changes"
    required: true
    manual: false
---

# Add terminal.shell, shell_args, scrollback, font_family and font_size settings, applied by the terminal panel, with a project file unable to set the shell (Mac T578)

Filed on argon from Mac T578 (card ~/coordination/argon-cards/T578.json rev 1). Mac task uid 01m4s6qycb1dd7fxkq0vceyscv. Build on argon main after its sync of mac/main 3eec614a: Mac T540 (editor.auto_save) in that stack touches the same fe-config settings.rs, schema and template. Title the host commit "... (Mac T578)".

## Problem

The terminal panel (`crates/fe-workbench/src/terminal_panel.rs`) always runs the user's login shell
(`TerminalPanel::shell` returns `None` outside tests), keeps `TerminalOptions::DEFAULT_SCROLLBACK` (10,000)
lines, and draws in the editor's font and size (`style(theme)`). `Settings` (`crates/fe-config/src/settings.rs`)
has no `terminal` section, so a user can't pick a shell (fish, nu, zsh without login), a scrollback length, or a
terminal font different from the editor's. Every terminal (VS Code's `terminal.integrated.*`, Zed's `terminal`)
has these. fe-terminal's `TerminalOptions` already takes `shell: Option<(String, Vec<String>)>` and
`scrollback`, and fe-terminal-view's `TerminalStyle` already takes `font_family` and `font_size`, so this is
settings plus plumbing; no fe-terminal or fe-terminal-view change.

## Change

**fe-config** (`settings.rs`, re-exported from `lib.rs`): a `terminal` table, `TerminalSettings`, on `Settings`,
`#[serde(default)]`, documented like `EditorSettings`:

- `shell: String`, default `""`: the program the terminal runs; empty runs the login shell as today.
- `shell_args: Vec<String>`, default `[]`: its arguments; ignored when `shell` is empty.
- `scrollback: usize`, default 10,000, `#[schemars(range(min = 0, max = 100000))]`: lines kept above the screen.
- `font_family: Option<String>`, unset (not serialized), `#[schemars(length(min = 1))]`: unset uses the
  editor's font.
- `font_size: Option<f64>`, unset (not serialized), `#[schemars(range(min = 6.0, max = 72.0))]`: unset uses
  the editor's size.

A project file may not choose the program a folder's terminal runs: add `ProjectFilter::new("terminal",
drop_terminal_shell)` in `options()`, which removes `terminal.shell` and `terminal.shell_args` from a project
layer with a warning `PROJECT_TERMINAL_SHELL` ("terminal.shell and terminal.shell_args in project files are
ignored: a folder can't choose the program its terminal runs") at each key, as `drop_theme` does for `theme`.
The other `terminal` keys apply from a project file. Regenerate `settings.schema.json` and add a commented
`[terminal]` block with every key to `settings-template.toml`; extend `tests/template.rs` so it checks the
terminal keys like the editor's.

**fe-workbench:** `TerminalPanel::set_settings(&TerminalSettings, cx)` stores them. `start` builds
`TerminalOptions` from them (`shell: None` when `shell` is empty, else `Some((shell, shell_args))`;
`scrollback`), with the test-only `set_shell` override still winning. `style()` takes the settings' font family
and size over the theme's when set, and a change of either restyles the running session the way `set_theme`
does. A change of shell, args or scrollback applies to the next session; the running one keeps its shell.
`Workbench::set_terminal_settings` forwards to the panel; `Workbench::reload_settings` calls it with
`event.settings.terminal` when it differs from `event.previous.terminal`.

**fastedit:** `new_workbench` in `crates/fastedit/src/main.rs` calls `workbench.set_terminal_settings` with
`settings.terminal` beside `set_status_bar_visible`, so a window's first terminal uses them.

No keys and no menus change.

## Tests

- `crates/fe-config/tests/terminal_settings.rs`: defaults; a user file setting each key round-trips; out-of-range
  `scrollback` and `font_size` are diagnostics with the last good value kept; a project file's `shell` and
  `shell_args` are dropped with `PROJECT_TERMINAL_SHELL` warnings while its `scrollback` applies.
- `crates/fe-workbench/src/terminal_settings_tests.rs` (`#[path]` from `terminal_panel.rs`): with `shell =
  "/bin/sh"` and `shell_args = ["-c", "printf configured; exec cat"]` (no `set_shell` override), the started
  session's screen shows `configured`; a reload changing `font_size` restyles the running session (its style's
  `font_size` changes) and keeps the same session; a reload changing `shell` keeps the running session and the
  next session started runs the new one.
