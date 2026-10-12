---
task_uid: "01m4rwrnxryfw8p2672g2gysd1"
task: "T90"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-config test auto_save_setting::defaults_and_parse: auto_save defaults to off and auto_save_delay to 1000; each of off, after_delay, on_focus_change, on_window_change, on_insert_leave parses from settings.toml; an unknown value and a delay of 50 are reported as settings problems, the defaults kept"
    required: true
    manual: false
  - id: "C2"
    text: "settings.schema.json describes both with their defaults and the delay's range, the template lists both commented out, and cargo test -p fe-config passes (the template test included); git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Add the editor.auto_save and editor.auto_save_delay settings: off, after_delay, on_focus_change, on_window_change and on_insert_leave (Mac T540)

## Problem

The user (2026-10-12) asked for "auto-save too": a setting `editor.auto_save` = off | after_delay (ms, default 1000) |
on_focus_change | on_window_change, matching VS Code (`files.autoSave`, `files.autoSaveDelay`). The vim-mode
requirement (coordinator, same day) adds an `on_insert_leave` mode, as Neovim's auto-save plugins trigger on
InsertLeave. `EditorSettings` (`crates/fe-config/src/settings.rs` :119) has no such key. T466 adds two other keys to the
same three files, so this follows it.

## Change

In `EditorSettings`, with doc comments that become the schema's descriptions:

- `auto_save: AutoSave`, default `off`, an enum serialised snake_case: `off`; `after_delay` ("Save a changed file
  `auto_save_delay` ms after the last edit; in vim insert mode, once insert mode ends"); `on_focus_change` ("Save a
  changed file when its editor loses focus or the window does"); `on_window_change` ("Save changed files when the
  window loses focus"); `on_insert_leave` ("With vim on, save a changed file when insert mode ends and after each
  normal-mode change; with vim off, as on_focus_change").
- `auto_save_delay: u64` milliseconds, default 1000, range 100 to 60000 (`#[schemars(range(...))]`); used only by
  `after_delay`.

Regenerate `crates/fe-config/settings.schema.json`, and add both to `crates/fe-config/settings-template.toml`
commented out with their defaults, beside T466's keys, as the template test requires. Nothing reads them yet.

## Tests

New file `crates/fe-config/tests/auto_save_setting.rs`.
