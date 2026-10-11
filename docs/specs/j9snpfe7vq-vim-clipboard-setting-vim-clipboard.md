---
task_uid: "01m4ewv4460cbkxvj9snpfe7vq"
task: "T223"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test vim_clipboard_setting (tests/vim_clipboard_setting.rs): Settings::default().vim.clipboard is empty; [vim] clipboard = \"unnamedplus\" loads with no diagnostics; clipboard = \"bogus\" gives one diagnostic and reads as empty; the template still loads to Settings::default(); settings.schema.json is regenerated and its golden test passes; tests/vim_setting.rs passes unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::clipboard_tests::system_clipboard_in_the_editor: with vim on, \"+yy puts the row and its line break on the test app's clipboard; text written to that clipboard then \"*p pastes it below; with the vim.clipboard setting applied as unnamedplus, yy fills the clipboard and cmd-v in insert mode pastes the row; after a settings reload back to \"\", applied through the same fe-gpui function that fastedit's settings observer (main.rs follow_settings) calls on reload, yy leaves the clipboard unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Scope: nothing under crates/fe-vim changes; no Cargo.toml or Cargo.lock change; no file outside the scope changes (the lane's docs/specs file excepted); fe-gpui's other vim tests pass unchanged"
    required: true
    manual: false
---

# Vim clipboard setting: vim.clipboard = unnamedplus applied in the editor at startup and on reload, and the + and * registers checked there

# Vim clipboard setting: vim.clipboard = "unnamedplus" applied in the editor, and "+ and "* checked there

Milestone M7 "Vim", second batch (resized), card 07b: the setting and editor half of the old card 7.
Read 07a (`Registers::set_clipboard_unnamed`) and T161's spec (the `vim.enabled` setting).

**Clean room** as every M7 card: behaviour from `:help clipboard-unnamedplus`.

## What exists

fe-gpui's `ViewHost` (used by `vim.rs`'s `dispatch_vim`) implements `read_clipboard` /
`write_clipboard` with GPUI's clipboard as plain strings. `fastedit`'s `main.rs` applies
`vim.enabled` at startup and on reload through `fe_gpui::set_vim_enabled`.

## Change

### The setting (`crates/fe-config/src/settings.rs`, `settings.schema.json`, `settings-template.toml`)

`[vim] clipboard = ""` (default, nvim's) or `"unnamedplus"`. Unknown values are a diagnostic and
read as `""`. Hot-reloads like `vim.enabled`.

### The editor (`crates/fe-gpui/src/vim.rs`, `crates/fe-gpui/src/lib.rs`, `crates/fastedit/src/main.rs`)

fe-gpui applies `vim.clipboard` to the shared `Registers` at startup and on reload, through the same
path `vim.enabled` takes: a `set_vim_clipboard` in `vim.rs` beside `set_vim_enabled`, re-exported
from `lib.rs` by one added line (the only `lib.rs` change).

## Tests (new files)

- `crates/fe-config/tests/vim_clipboard_setting.rs`.
- `crates/fe-gpui/src/vim/clipboard_tests.rs`, declared from `vim.rs` as its other test modules.

## Size

About 100 non-test lines.

## Out of scope

Persisting the setting from a toggle (T170's mechanism covers only `vim.enabled`); `"unnamed"` may be
accepted as a synonym if trivial.

## Decided (lead defaults; the user may revisit)

- **Default off, as nvim's.** Turning it on is one settings line.
- **`cmd-c`/`cmd-v` stay the editor's** and don't touch vim registers; with `unnamedplus` they meet
  through the clipboard anyway.
