---
task_uid: "01m4nzg4vfn8gq18qxvjty37md"
task: "T369"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test vim_jump_on_s_setting: Settings::default().vim.jump_on_s is false; a user file with [vim] jump_on_s = true sets it with no diagnostics; USER_SETTINGS_TEMPLATE loads to Settings::default(); schema_matches_golden and template_is_all_defaults pass"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::jump_setting_tests::applies_to_editors: after set_vim_jump_on_s(cx, true) an open editor and one opened afterwards both start a jump on s (vim.pending_kind jump), and after set_vim_jump_on_s(cx, false) s is cl again"
    required: true
    manual: false
  - id: "C3"
    text: "main.rs calls fe_gpui::set_vim_jump_on_s(cx, settings.vim.jump_on_s) at startup beside set_vim_clipboard and in follow_settings' observer on every reload past the ptr_eq return (the source lines quoted in the decision file); nothing under crates/fe-vim changes"
    required: true
    manual: false
---

# Add a vim.jump_on_s setting that lets the jump take s and S, applied in the editor at startup and on reload

# Add a vim.jump_on_s setting that lets the jump take s and S, applied in the editor at startup and on reload

## Problem

**The user's decision (2026-10-11) on jump keys:** "Other default keys but allow opt-in to replace s / S".
- The jump defaults to `gs`/`gS` (the motion card).
- The fe-vim session card adds `VimState::set_jump_on_s(bool)`, but no user setting reaches it.

This card is kept apart from the fe-vim cards because it touches the hot settings, host and fe-gpui files
(`settings.rs`, `settings.schema.json`, `main.rs`, `fe-gpui/src/vim.rs`) for a few lines.

## Change

It follows T223's `vim.clipboard`.

**Setting** (`crates/fe-config/src/settings.rs`):
- Add `jump_on_s: bool` to `VimSettings`, default `false`.
- Doc: "true: `s` and `S` start the forward and backward jump, as leap and flash map them, instead of
  changing a char or a line (`cl` and `cc` still do). `gs` and `gS` start it either way."
- Regenerate `crates/fe-config/settings.schema.json` (`UPDATE_GOLDEN=1`, checked by
  `schema_matches_golden`).
- Add a commented `# jump_on_s = false` with its doc line to the `[vim]` section of
  `crates/fe-config/settings-template.toml` (checked by `template_is_all_defaults`).

**fe-gpui** (`crates/fe-gpui/src/vim.rs`, `crates/fe-gpui/src/lib.rs`):
- `pub fn set_vim_jump_on_s(cx, on)` keeps the value on the `Vim` global and sets it on every live
  editor's `VimState`.
- `sync_vim` sets it on each `VimState` it creates, so editors opened later and vim toggled back on get it
  too.
- Re-export it from `lib.rs` beside `set_vim_clipboard`.

**Host** (`crates/fastedit/src/main.rs`):
- At startup, call `fe_gpui::set_vim_jump_on_s(cx, settings.vim.jump_on_s)` beside `set_vim_clipboard`.
- In `follow_settings`' observer, after the `ptr_eq` return, make the same call with the reloaded value on
  every reload. There's no toggle to keep.

## Tests (new files)

- `crates/fe-config/tests/vim_jump_on_s_setting.rs`.
- `crates/fe-gpui/src/vim/jump_setting_tests.rs` (one `mod` line in `vim.rs`, as `clipboard_tests`).

## Out of scope

- Other jump settings (labels, backdrop, style), which wait until a card needs them.
- Keeping project files from setting the key.
