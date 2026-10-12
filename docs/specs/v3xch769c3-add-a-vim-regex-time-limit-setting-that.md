---
task_uid: "01m4r0dm27e4v5bhv3xch769c3"
task: "T40"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test vim_regex_time_limit_setting (tests/vim_regex_time_limit_setting.rs): Settings::default().vim.regex_time_limit is 2000; a user file with [vim] regex_time_limit = 500 sets 500 with no diagnostics; regex_time_limit = 0 is an Error diagnostic and leaves 2000; USER_SETTINGS_TEMPLATE loads to Settings::default(); schema_matches_golden and template_is_all_defaults pass"
    required: true
    manual: false
  - id: "C2"
    text: "main.rs calls fe_gpui::set_vim_time_limit(Duration::from_millis(settings.vim.regex_time_limit)) at startup beside set_vim_enabled, and in follow_settings' observer on every reload past the ptr_eq return; crates/fe-gpui/src/lib.rs re-exports fe_vim::search::set_time_limit as set_vim_time_limit (the source lines quoted in the decision file)"
    required: true
    manual: false
  - id: "C3"
    text: "Nothing under crates/fe-vim changes; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Add a vim.regex_time_limit setting that the editor applies to fe-vim's search and :s time limit at startup and on reload (Mac T362)

Filed on argon from Mac T362 (card ~/coordination/argon-cards/T362.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12-argon-i.md). Mac task uid 01m4ny2z5g50ce68hcmwe0dc93. Its dependencies (Mac T310, Mac T223, Mac T245) are already on argon main (synced to mac/main 6d07f26), so they are not recorded here. Built on argon main synced to mac/main f5bee8c. Title the host commit "... (Mac T362)".

## Problem

T310 stops `:s` and vim's searches once they pass a wall-clock limit:
`fe_vim::search::DEFAULT_TIME_LIMIT` (2 s), changed process-wide through `fe_vim::search::set_time_limit`.
Nothing outside tests calls `set_time_limit`, so the user can't change the limit.

**Decided** (user, 2026-10-11): "make the limit a user setting". This card is kept apart from T310 so that
T310 stays inside fe-vim. The setting touches fe-config, fe-gpui and the host, and fe-vim stays free of GPUI
(`scripts/check-no-gpui.sh`).

## Change

The setting follows T223's `vim.clipboard`, and is applied at startup and on every reload through the same
path as `vim.enabled`.

**Setting** (`crates/fe-config/src/settings.rs`): add `regex_time_limit: u64` to `VimSettings`, in
milliseconds, `#[schemars(range(min = 1))]`, default `2000` (the same as `DEFAULT_TIME_LIMIT`; fe-config doesn't
depend on fe-vim, so write the number with a doc comment pointing at it). Doc: "How long, in milliseconds, a
vim search or :s may run on one command before it stops with Interrupted and changes nothing (nvim's
'redrawtime' is the same default)." A value below 1 is a range error, as with `lsp.timeouts`: the file keeps
its last good table. Regenerate `crates/fe-config/settings.schema.json` (`UPDATE_GOLDEN=1`, checked by
`schema_matches_golden`). Add a commented `# regex_time_limit = 2000` with its doc line to the `[vim]` section
of `crates/fe-config/settings-template.toml` (checked by `template_is_all_defaults`).

**fe-gpui** (`crates/fe-gpui/src/lib.rs`): one added line,
`pub use fe_vim::search::set_time_limit as set_vim_time_limit;`, so the host reaches it without depending on
fe-vim. Don't add a wrapper in `vim.rs`: T313 and T231 edit that file.

**Host** (`crates/fastedit/src/main.rs`):

- at startup, beside `fe_gpui::set_vim_enabled(cx, settings.vim.enabled)`:
  `fe_gpui::set_vim_time_limit(Duration::from_millis(settings.vim.regex_time_limit))`;
- in `follow_settings`' observer, after the `ptr_eq` early return: the same call with the reloaded
  settings, on every reload. There's no toggle to keep, so it doesn't wait for this key to change.

The value comes from `GlobalSettings<Settings>`, the user's file and the launch root's project layer, as
`vim.enabled` does. fe-vim's limit is process-wide, so every window gets the same value.

## Tests

- `crates/fe-config/tests/vim_regex_time_limit_setting.rs`, a new file.
- No fe-gpui or host test: `set_time_limit` is process-wide, and setting it in fe-gpui's unit-test binary
  would change the limit for every test running alongside. The decision file quotes the host's two call
  sites instead.

## Out of scope

- Search highlighting's limit (its own card; it reads `time_limit()` too, so this setting governs it once
  that card lands).
- Keeping project files from setting the key.
