---
task_uid: "01m6rmdawx10f91ec922xcrhn4"
task: "T479"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test (tests/wrap_column_settings.rs) wrap_settings_default_and_validate: wrap_column defaults 0 and wrap_mode viewport; 10 and 600 are rejected or clamped to 20..=500; an unknown mode is rejected; template and schema list both"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test wrap_column_tests::column_mode_wraps_at_n_columns_in_a_wider_window: soft wrap on, mode column, N=40, viewport 120 columns wide: a 100-char row wraps into 3 rows at column 40"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test wrap_column_tests::bounded_mode_takes_the_smaller_width: the same row wraps at the viewport width in a 30-column window and at 40 in a 120-column window"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test wrap_column_tests::soft_wrap_off_ignores_the_column: with soft_wrap false nothing wraps whatever the column and mode; alt-z still toggles"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test wrap_column_tests::changing_the_setting_rewraps_on_reload: reloading with a new column rewraps open editors"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Wrap at a fixed column: editor.wrap_column and editor.wrap_mode (viewport, column, bounded) for soft wrap (Mac T1402)

Lent from Mac T1402 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

Soft wrap is only on/off at the window edge (`editor.soft_wrap`). VS Code's `editor.wordWrap: wordWrapColumn` and `bounded` wrap at a fixed column, or at min(viewport, column), which people use for prose and Markdown at 80 or 100 columns. Verified missing: no wrap column anywhere in `fe-config` settings or the display wrapping code.

Linux-OK: wrapping is headless in fe-display; a width computation test, no screenshot.

## Change

- Settings: `editor.wrap_column` (integer, 0 = off, default 0; valid 20..=500, others rejected by the schema and clamped) and `editor.wrap_mode` ("viewport" default, "column", "bounded"). `column` wraps at N columns regardless of the window (text beyond the viewport scrolls); `bounded` wraps at min(viewport, N). Only meaningful when soft wrap is on; the Settings tab entry says so. `crates/fe-config/src/settings.rs`, `settings-template.toml`, `settings.schema.json`.
- `crates/fe-gpui/src/view.rs` (where `set_soft_wrap` is applied on settings reload, ~line 790, and the viewport width reaches the display map's `wrap_width`) and, only if the width cap must live in the map, `crates/fe-display/src/wrap_map.rs`: compute the effective wrap width as chars-times-advance of the monospace cell (use the cell width the editor already measures), per editor. Changing the setting re-wraps on reload. alt-z (soft wrap toggle) behaves as today.
- Per-language override works for free if `[language.<id>]` tables already cover `editor.*` (check `settings.rs`); if not, do not add them here.

## Out of scope

Hard wrap on type, rulers, reflow command (Rewrap).

## Conflicts

Settings triple shared with the format-on-type and Start page cards; the wrap code in fe-display has no queued work (T975 jump-scroll prepaint is in the same neighbourhood: inbox, not running).
