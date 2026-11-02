---
task_uid: "01m6enynvf3m2z2yda1vnfeg94"
task: "T1317"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test editor_inline_diagnostics_setting passes: editor.inline_diagnostics defaults to warning, parses off, error, warning and all, and a bad value is a settings diagnostic"
    required: true
    manual: false
  - id: "C2"
    text: "Test inline_diagnostics_tests::the_most_severe_message_per_row_shows_and_the_level_filters (cargo nextest run -p fe-gpui) passes: error, warning and all each show only diagnostics at or above their level, the first line of the message, the earliest range winning a severity tie, off shows nothing"
    required: true
    manual: false
  - id: "C3"
    text: "Test inline_diagnostics_tests::a_wrapped_row_draws_on_its_last_display_row_and_is_cut_at_the_viewport (cargo nextest run -p fe-gpui) passes: the message starts one em after the last glyph, ends in an ellipsis at the right edge (less a right-hand minimap), and is not drawn with under 8 chars of room, on a folded row or past the horizontal scroll"
    required: true
    manual: false
  - id: "C4"
    text: "Test inline_diagnostics_tests::the_overlay_changes_neither_the_layout_nor_the_minimap (cargo nextest run -p fe-gpui) passes: with the setting on and off the frame's row positions, caret x and the minimap's painted tiles are identical, and a click or hover over the message reaches the text below"
    required: true
    manual: false
  - id: "C5"
    text: "Test inline_diagnostics_tests::only_visible_diagnostic_rows_are_shaped (cargo nextest run -p fe-gpui) passes: with 10000 diagnostics and 50 visible rows, at most one shape per visible diagnostic row"
    required: true
    manual: false
---

# Show the highest-severity diagnostic message dimmed after the end of its row, behind editor.inline_diagnostics

## Problem

Diagnostics show as squiggles (`diagnostic_marks`, crates/fe-gpui/src/element.rs), a gutter dot, hover and the Problems
panel, but the message is only visible on hover. The user approved (2026-11-02) Error Lens style inline messages:
after the last character of a row with a diagnostic, the highest-severity message, dimmed.

## Change

- Setting `editor.inline_diagnostics` in `EditorSettings` (crates/fe-config/src/settings.rs): a snake_case enum
  `off | error | warning | all`, default `warning`. `error` shows errors only, `warning` errors and warnings, `all`
  adds information and hint. Add it to settings.schema.json (regenerate) and settings-template.toml. It reaches the
  view through the same settings-apply function as `cursor_surrounding_lines` (crates/fe-gpui/src/view.rs), so a
  change applies live.
- New crates/fe-gpui/src/inline_diagnostics.rs, shaped like color_swatches.rs (`layout(&frame, ...)` then `paint`),
  called from crates/fe-gpui/src/element.rs beside `diagnostic_marks`, reusing the `view.model.diagnostics()` vector
  already fetched there. It is a paint-time overlay, not a display-map inlay: it never changes the text, wrapping,
  column maths, carets or hit-testing, and the mouse passes through it (hover on the squiggle is unchanged).
- Per buffer row with a shown diagnostic at or above the setting's level: the most severe diagnostic whose range
  touches the row (ties: the earliest range start), its message's first line, in `EditorTheme::diagnostic_color`
  at about 60% alpha, in the editor font, one em after the row's last glyph. A soft-wrapped row draws it on its last
  display row only. It is cut with an ellipsis at the viewport's right edge (less the minimap's width when it is
  on the right); under about 8 chars of room it is not drawn. A row whose end is scrolled out of view, a folded
  row, and a row served as a long-row slice draw nothing.
- Performance: only the frame's rows are visited, once; each shaped message is cached by (text, colour) for the
  frame, and at most one shape per visible diagnostic row.
- The minimap is unaffected (it draws from text and its decoration sweep, not from this overlay); no new decoration
  provider and no new theme keys.

Tests go in new crates/fe-gpui/src/inline_diagnostics_tests.rs (declared from inline_diagnostics.rs) and
crates/fe-config/tests/editor_inline_diagnostics_setting.rs.
