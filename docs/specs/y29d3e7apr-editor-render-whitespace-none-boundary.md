---
task_uid: "01m5qp0n845zq2yfy29d3e7apr"
task: "T1154"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config whitespace_rulers_settings: render_whitespace defaults to selection, rulers to empty; all five values and a rulers list parse; template and schema list both; every bundled theme sets the whitespace and ruler colours"
    required: true
    manual: false
  - id: "C2"
    text: "whitespace_tests::modes: for 'a  b\\t c   ' with a selection over 'b\\t', all marks every space and tab, boundary skips none but the single-space word gap rules, selection marks only the selected whitespace, trailing only the last three spaces, none nothing (mark counts and positions asserted from the frame layout)"
    required: true
    manual: false
  - id: "C3"
    text: "whitespace_tests::wrap_and_fold: marks on a soft-wrapped row sit on the right screen rows and none are produced for folded rows or rows past the frame"
    required: true
    manual: false
  - id: "C4"
    text: "rulers_tests::columns: rulers = [80, 120] give two lines at 80 and 120 monospace advances, shift left with horizontal scroll, and an empty list gives none"
    required: true
    manual: false
  - id: "C5"
    text: "Editing settings.toml changes both live; existing indent-guide and theme tests pass unedited; cargo nextest run -p fe-config -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# editor.render_whitespace (none/boundary/selection/trailing/all) and editor.rulers settings, painted as VS Code does

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's `editor.renderWhitespace` (none, boundary, selection, trailing, all; default selection) and `editor.rulers` (vertical lines at columns) are missing. Verified: no whitespace or ruler setting in `crates/fe-config/src/settings.rs` (EditorSettings, line 396) and no painter for either.

## Current state
- Editor settings are plain fields in `EditorSettings` (settings.rs:396-470), mirrored in `settings-template.toml` and `settings.schema.json`, copied to the view in `crates/fe-gpui/src/lib.rs` / `theme.rs:25-30` (see how `indent_guides` and `font_ligatures` flow; `crates/fe-config/tests/font_ligatures_setting.rs` and `indent_guides_setting.rs` are the test pattern).
- Indent guides are the nearest painter: `crates/fe-gpui/src/element/indent_guides.rs` lays out rects per frame from `FrameLayout`, tested through `view.layout` (`indent_guides_tests.rs`), so no golden is needed. Theme colour keys live in `crates/fe-config/src/theme.rs:95-110` (`indent_guide`, `indent_guide_active`) and are set in all six bundled themes in `crates/fe-config/themes/` (a test checks each, `crates/fe-config/tests/indent_guide_colors.rs`).

## Change
- Settings: `editor.render_whitespace` (enum `none|boundary|selection|trailing|all`, default `selection`) and `editor.rulers` (array of columns, default empty). Colour keys `whitespace` (VS Code editorWhitespace.foreground) and `ruler` (editorRuler.foreground) in the theme's `[editor]`, set in all six bundled themes, faint.
- Painting, new `crates/fe-gpui/src/element/whitespace.rs`: a space is a centered dot, a tab an arrow spanning its advance, in the `whitespace` colour. `all`: every space and tab; `boundary`: all except a single space between two words; `selection`: only whitespace inside a selection range; `trailing`: only whitespace after a row's last non-blank char; `none`: nothing. Soft-wrapped rows, folds and blocks are handled as the indent guides do (draw only for frame rows in view); rows over the long-row limit are skipped.
- Rulers, new `crates/fe-gpui/src/element/rulers.rs`: a one-pixel vertical line spanning the text area at each column (x = column * monospace char width, minus horizontal scroll), under the text, in the `ruler` colour; columns <= 0 ignored.
- Both flow live when settings.toml changes, like the other editor settings.

## Tests
New `crates/fe-config/tests/whitespace_rulers_settings.rs`, `crates/fe-gpui/src/element/whitespace_tests.rs`, `crates/fe-gpui/src/element/rulers_tests.rs` (declared from their painter modules).
