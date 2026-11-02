---
task_uid: "01m6ce8dbevx42abjw9q46za9j"
task: "T403"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test vim_quick_scope_setting passes: vim.quick_scope defaults to on_keys, loads off and always, and another value is a diagnostic"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config --test quick_scope_colors passes: every bundled theme sets quick_scope_primary and quick_scope_secondary at 3:1 or more against its background; editor_palette and minimap_theme_keys still pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test element::quick_scope::tests::marks_follow_the_mode passes: always lays out both colours' underlines under the targets in normal and visual mode; on_keys lays out none until f (forward targets only) or F (backward only) waits, and none once the char is typed; off and insert mode lay out none"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fastedit/src/main.rs applies vim.quick_scope at startup and in follow_settings"
    required: true
    manual: false
---

# Underline quick-scope's f and F targets on the vim caret's row in quick_scope_primary and quick_scope_secondary theme colours, behind vim.quick_scope = off, on_keys (default) or always (Mac T1296)

## Problem

The user approved quick-scope on 2026-11-01: highlight the caret row's `f`/`F` targets in primary and secondary
colours, optionally only after pressing `f`/`F`/`t`/`T`, with a setting and theme tokens. The targets card
(depends-on) computes them (`fe_vim::quick_scope::targets`, `Pending::find_direction`); nothing paints them.

## Change

1. Setting (`crates/fe-config/src/settings.rs`, `VimSettings`): `quick_scope: VimQuickScope`, an enum serialised
   `"off"`, `"on_keys"` (default) or `"always"`. Doc: "Underlines the first char of each word on the caret's row
   that `f` or `F` reaches in one or two presses, as quick-scope does: `always` in normal and visual mode;
   `on_keys` only while `f`, `F`, `t` or `T` waits for its char, in that direction." Schema (regenerate) and
   `settings-template.toml`.
2. Colours: `EditorColors` keys `quick_scope_primary` and `quick_scope_secondary` in `crates/fe-config/src/theme.rs`,
   after `vim_yank_highlight` (before the minimap block, which must stay last), set in every bundled theme
   (`crates/fe-config/themes/*.toml`): quick-scope's own `#afff5f` and `#5fffff` in the dark themes where they
   read well, else each theme's own green and cyan; every one at least 3:1 contrast against that theme's
   `background`. `EditorTheme` fields (`crates/fe-gpui/src/lib.rs`) filled in `crates/fe-gpui/src/theme.rs`.
3. fe-gpui: `pub fn set_vim_quick_scope(cx: &mut App, mode)` in `crates/fe-gpui/src/vim.rs` (stored on the `Vim`
   global; re-exported from `crates/fe-gpui/src/lib.rs`). A new `crates/fe-gpui/src/element/quick_scope.rs` lays
   out and paints, hooked into `crates/fe-gpui/src/element.rs` beside `rulers::layout`/`whitespace::layout` and
   their paint: for the primary caret of a focused editor with vim on, in normal or a visual mode (`always`), or
   while `find_direction()` is `Some` (`on_keys`, that direction only; `always` also narrows to it then), a
   2 px underline under each target's cell, in the primary or secondary colour, painted after the text. Underline
   only, no glyph recolour: recolouring would re-shape the row on every caret move (`foreground_spans` re-lays
   out the frame). Nothing in insert, replace or cmdline mode, or with `off`. Rows folded or scrolled off get
   nothing; a soft-wrapped row's targets land on their display rows.
4. `crates/fastedit/src/main.rs`: apply the setting at startup and in `follow_settings`.

Tests: new `crates/fe-config/tests/vim_quick_scope_setting.rs`, new `crates/fe-config/tests/quick_scope_colors.rs`,
and new `crates/fe-gpui/src/element/quick_scope_tests.rs` (declared from `quick_scope.rs`), asserting the laid-out
marks (offsets, colours) for each mode, and none in insert mode.

Notes: T659 and T1168 (lent to neon) edit the same settings/wiring files; T198 (inbox) lists `element.rs`.
If T1273's token probe lists `EditorTheme` fields by hand, add the two there too.
