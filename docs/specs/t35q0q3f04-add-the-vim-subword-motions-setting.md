---
task_uid: "01m6c9t491rqj7fqt35q0q3f04"
task: "T268"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test vim_subword_motions_setting passes: vim.subword_motions defaults to false and a [vim] table with true loads as such"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::subword_setting_tests::the_setting_reaches_every_editor passes: after set_vim_subword_motions(cx, true) an open editor's w on fooBar stops at B and so does one opened later; false puts w back"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fastedit/src/main.rs applies vim.subword_motions at startup and in follow_settings, as it applies vim.replace_with_register"
    required: true
    manual: false
---

# Add the vim.subword_motions setting (default false), applied live to every editor, which makes w b e ge step by camelCase and snake_case parts (Mac T1294)

## Problem

The subword motions card adds `VimState::set_subword_motions` (off by default: `w b e ge` move by words). The
user can't switch it on: there is no setting. The user approved subword motions on 2026-11-01 as opt-in.

## Change

- `crates/fe-config/src/settings.rs`, `VimSettings`: `subword_motions: bool`, default `false`. Doc: "true: `w`,
  `b`, `e` and `ge` stop at each part of a camelCase, PascalCase, snake_case or digit-joined name, as
  vim-wordmotion does; `W B E gE` still move by WORDs. `iS` and `aS` select a subword either way." Add it to
  `settings.schema.json` (regenerate) and `settings-template.toml`.
- `crates/fe-gpui/src/vim.rs`: `pub fn set_vim_subword_motions(cx: &mut App, on: bool)`, stored on the `Vim`
  global and applied to every editor's `VimState` and to editors opened later, exactly as
  `set_vim_replace_with_register` is; re-exported from `crates/fe-gpui/src/lib.rs`.
- `crates/fastedit/src/main.rs`: apply it at startup and in `follow_settings`, beside
  `set_vim_replace_with_register`.

Tests: new `crates/fe-config/tests/vim_subword_motions_setting.rs` and new
`crates/fe-gpui/src/vim/subword_setting_tests.rs` (declared from `crates/fe-gpui/src/vim.rs`), modelled on
`replace_register_setting_tests.rs`.

Note: T659 and T1168 (lent to neon) edit the same settings and wiring files; expect a textual merge when neon's
stack lands.
