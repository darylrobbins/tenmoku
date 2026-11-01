---
task_uid: "01m6ce89gwp1ncx5058pxx9vnn"
task: "T401"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test vim_highlight_yank_setting passes: vim.highlight_yank defaults to true and vim.highlight_yank_ms to 150, a [vim] table with false and 400 loads as such, and the schema gives highlight_yank_ms minimum 1 and maximum 5000"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config --test vim_yank_highlight_colors passes: every bundled theme sets vim_yank_highlight and a user theme without it gets the bundled palette's; the editor_palette, ember_theme and minimap_theme_keys tests still pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::yank_flash_setting_tests::the_settings_shape_the_flash passes: after set_vim_highlight_yank(cx, false, ..) yiw adds no decoration; with true and 400 ms a Highlight with Tint::Yank covers the word at 399 ms and is gone at 401 ms; vim::clipboard_tests::yank_flash_tests::a_yank_flashes_its_text still passes"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fastedit/src/main.rs applies vim.highlight_yank and vim.highlight_yank_ms at startup and in follow_settings, as it applies vim.which_key"
    required: true
    manual: false
---

# Add vim.highlight_yank and vim.highlight_yank_ms settings and a vim_yank_highlight theme colour for the vim yank flash, which is always on, 150 ms, in the faded cursor colour today (Mac T1288)

## Problem

The user approved highlighted yank (vim-highlightedyank, nvim's `vim.hl.on_yank`) as a built-in vim feature on
2026-11-01, "default on, ~150-200 ms, a setting for on/off and duration, a theme token for the colour".

The flash itself already exists (Mac T889, landed via a host stack): `fe_vim::VimState::last_yank`
(`crates/fe-vim/src/state.rs:224`) and `EditorView::flash_new_yank` in `crates/fe-gpui/src/vim.rs` (~line 1313)
put the yanked ranges in a `DecorationSet` as `DecorationKind::Highlight` with `Tint::Accent` for the
hard-coded `const YANK_FLASH: Duration = 150 ms` (`crates/fe-gpui/src/vim.rs:159`). What's missing:

- it can't be turned off or lengthened: there is no setting;
- its colour is `Tint::Accent`, which `tint_color` (`crates/fe-gpui/src/element/decorations.rs:73`) resolves to
  `theme.cursor`, faded: a theme can't colour it on its own.

## Change

1. Settings (`crates/fe-config/src/settings.rs`, `VimSettings`): `highlight_yank: bool` (default `true`; doc:
   "true: a yank briefly highlights the text it took, as nvim's vim.hl.on_yank does") and
   `highlight_yank_ms: u64` (default `150`, nvim's default; `#[schemars(range(min = 1, max = 5000))]`). Add both
   to `settings.schema.json` (regenerate) and `settings-template.toml` beside `which_key_delay_ms`.
2. Theme colour: a new `EditorColors` key `vim_yank_highlight` in `crates/fe-config/src/theme.rs`, declared right
   after `jump_label_foreground` (the minimap block must stay last: `tests/minimap_theme_keys.rs` asserts it).
   Every bundled theme in `crates/fe-config/themes/*.toml` sets it (`tests/editor_palette.rs` and
   `tests/ember_theme.rs` require every key in every bundled theme): each theme's `cursor` colour at the alpha
   the Highlight fade paints today, so the default look doesn't change. `EditorTheme` (`crates/fe-gpui/src/lib.rs`)
   gains `vim_yank_highlight: Hsla`, filled in `crates/fe-gpui/src/theme.rs`.
3. Tint: a new `Tint::Yank` in `crates/fe-editor/src/decorations.rs`; `crates/fe-gpui/src/element/decorations.rs`
   paints a `Highlight` with it in `theme.vim_yank_highlight` unfaded (as `diff_color` paints the diff tints), and
   `tint_color` maps it there too.
4. fe-gpui (`crates/fe-gpui/src/vim.rs`): `pub fn set_vim_highlight_yank(cx: &mut App, on: bool, duration:
   Duration)`, stored on the `Vim` global as `which_key`'s settings are; re-exported from `crates/fe-gpui/src/lib.rs`.
   `flash_new_yank` reads it: off, no decoration (the serial is still consumed, so turning it on later doesn't
   flash an old yank); on, the timer is the setting's duration instead of `YANK_FLASH` (keep the const as the
   default). The flash uses `Tint::Yank`.
5. `crates/fastedit/src/main.rs`: call `set_vim_highlight_yank` at startup and in `follow_settings`, as
   `set_vim_which_key` is called.

Tests: new `crates/fe-config/tests/vim_highlight_yank_setting.rs`, new
`crates/fe-config/tests/vim_yank_highlight_colors.rs`, and new `crates/fe-gpui/src/vim/yank_flash_setting_tests.rs`
declared from `crates/fe-gpui/src/vim.rs` beside `mod yank_flash_reset_tests;` (advance the executor's clock as
`yank_flash_tests.rs` does).

Notes: T659 and T1168 (lent to neon, blocked here) also edit settings.rs, the schema, the template, main.rs,
fe-gpui vim.rs and lib.rs; expect a textual merge when neon's stack lands. If T1273 has landed and its
`crates/fe-workbench/examples/screenshots/token_probe.rs` lists `EditorTheme` fields by hand, the new field goes
there too (a scope notice, not a reason to stop).
