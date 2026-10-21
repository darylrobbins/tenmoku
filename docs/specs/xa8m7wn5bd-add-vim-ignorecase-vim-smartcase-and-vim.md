---
task_uid: "01m55mhz81e1t8wdxa8m7wn5bd"
task: "T159"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test vim_search_settings_defaults_and_round_trip (top level, in crates/fe-config/tests/vim_search_settings.rs) passes: ignorecase and smartcase default false and incsearch true, each round-trips through settings JSON, and the schema documents all three."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::jump_setting_tests::search_setting_tests::a_live_change_reaches_open_editors (new crates/fe-gpui/src/vim/search_setting_tests.rs, declared in crates/fe-gpui/src/vim/jump_setting_tests.rs) passes: turning ignorecase on through the new setter makes an already open editor's /hello find Hello, off again doesn't, and Registers::incsearch() follows the incsearch setter."
    required: true
    manual: false
---

# Add vim.ignorecase, vim.smartcase and vim.incsearch settings, applied live to every editor's vim (Mac T894)

## Problem

The search-case card gives fe-vim 'ignorecase' and 'smartcase', and the incsearch card needs an on/off
switch, but settings.json has only `vim.enabled`, `vim.clipboard`, `vim.jump_on_s` and
`vim.regex_time_limit` (`VimSettings`, `crates/fe-config/src/settings.rs`). VSCodeVim exposes
`vim.ignorecase`, `vim.smartcase` and `vim.incsearch`. The user approved them in the "Vim parity" track on
2026-10-17.

## Change

- `VimSettings` gains `ignorecase` (default false), `smartcase` (false) and `incsearch` (true), nvim's
  defaults, each documented for the schema as the others are.
- `Registers` (`crates/fe-vim/src/register.rs`) gains `set_incsearch(bool)`/`incsearch()` next to the search
  case.
- fastedit applies them at startup and on every settings reload (`crates/fastedit/src/main.rs`, where
  `vim.jump_on_s` is applied) through new fe-gpui setters in `crates/fe-gpui/src/vim.rs`, exported from
  `crates/fe-gpui/src/lib.rs` as `set_vim_jump_on_s` is, which set them on the shared `Registers` every
  editor's vim uses; a change reaches open editors at once.

Tests: a new `crates/fe-config/tests/vim_search_settings.rs` (defaults, round trip, schema), and a new
`crates/fe-gpui/src/vim/search_setting_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/jump_setting_tests.rs` by `#[path = "search_setting_tests.rs"] mod
search_setting_tests;`, showing a live change makes an open editor's `/hello` find `Hello`.
