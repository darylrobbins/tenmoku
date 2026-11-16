---
task_uid: "01m7c5f66jw7s8rr8563rmz1pe"
task: "T1849"
rev: 5
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test a_click_on_row_7s_number_toggles_a_breakpoint_at_row_7_not_at_the_carets passes (cargo nextest run -p fe-gpui --test breakpoint_gutter), read back through the doc-hidden drawn_gutter API. Known limit (T1881, review F15): the screen-reader test a_screen_readers_click_on_a_row_toggles_it calls activate_breakpoint_row directly, not the row node's registered accessibility Click, which GPUI's test platform can't dispatch"
    required: true
    manual: false
  - id: "C2"
    text: "Tests breakpoints_draw_over_the_number_column_and_the_gutter_stays_81_px and they_coexist_with_the_diagnostic_dot_and_the_lightbulb pass (fe-gpui --test breakpoint_gutter), and cargo nextest run -p fe-gpui --test gutter_b3 passes unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_gutter::states_follow_verification_events passes against the fake (cargo nextest run -p fe-workbench debug_gutter): enabled, unverified with the adapter's message as tooltip, disabled, and the reported line for the session's lifetime. Known limits (T1881): review F12, pending verification of a file's changed lines is cleared by any DebuggerEvent::Breakpoints for the file, including an adapter's breakpoint event for another breakpoint, not only the answer to the replacement send; review F11, after a save between a breakpoint's own line and the line the adapter reported, the next refresh maps the reported line against the new disk baseline, so the moved glyph can leave its text"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_gutter::a_stop_bands_the_stopped_row_and_draws_the_arrow passes: Tint::DebugCurrent on the top frame's row with the arrow; the frame-banding seam D9 calls gives a caller frame Tint::DebugFrame; the band clears on resume. Known limits (T1881): review F11, after a save that moves lines, the next refresh places the stopped and caller frames by their original rows against the new disk baseline, so they can snap back off their text; review F16, a band covers its line's text range, so a newline typed inside a stopped or caller line stretches the band over two rows until the next refresh"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_gutter::the_context_menu_disables_enables_and_removes passes, its items named for assistive technology. Known limit (T1881, review F17): the menu takes no keys; Escape doesn't close it, and arrows and Enter reach the editor under it"
    required: true
    manual: false
  - id: "C6"
    text: "Tests in crates/fe-config/tests/debug_tints.rs (cargo nextest run -p fe-config --test debug_tints) pass: every bundled theme sets the two new editor keys to D0's values (Yohen and Hakuji) or its own, and a theme without them gets the fallbacks; cargo nextest run -p fe-config passes. Known limit (T1881, review F14): the contrast test composites the hover tile over the plain gutter, not over the cursor line, where enabled dots in Tenmoku Dark, Ember, Light and Monokai and Tenmoku Light's frame arrow fall just under 3:1"
    required: true
    manual: false
  - id: "C7"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_gutter.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE, showing every D6 state and both bands"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Breakpoints over the line-number column, the current-frame arrow, and the debug line tints in every theme (debugger D6)

# Breakpoints over the line-number column, the current-frame arrow, and the debug line tints in every theme (debugger D6)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

Breakpoints and the stopped line must be visible in the editor. B3's gutter is a
fixed 81 px, pinned by tests, so §5's separate lane no longer fits (§13).

## What to build (§5 "The gutter", "The current line"; §11 D6; §13; D0's section)

- **fe-gpui**: `set_breakpoints(version, Vec<BreakpointMark>)` with click and
  context-menu callbacks, like `set_gutter_annotations`; marks drawn **over the
  line-number column** as D0 specifies (no new lane; `GutterGlyph` and its mark slot,
  `crates/fe-gpui/src/view.rs:2146`, are untouched); hit-testing a click on the number
  column; tooltips; the current-frame arrow; `drawn_gutter` gains the marks for tests.
- **Tints**: `Tint::DebugCurrent` and `Tint::DebugFrame` in `crates/fe-editor/src/decorations.rs`,
  their arms in `tint_color` (`crates/fe-gpui/src/element/decorations.rs:65`), and
  **theme roles (§13)**: two `[editor]` keys (`EditorColors`) with fallbacks to the
  accent and muted tints, set in every bundled theme with D0's values for Yohen and
  Hakuji, as `vim_yank_highlight` was added; any size role D0 names goes in kiln-ui's
  `roles.rs`. A new `[editor]` key must be set in every theme that spells out all
  keys (tenmoku-dark/light/ember, vscode-monokai), before the minimap keys.
- **Workbench wiring** (`debug_gutter.rs`): store → marks, click → toggle, context
  menu Disable/Enable and Remove; D7's stopped frame → band and arrow; a seam to band
  a selected caller frame (D9 uses it).
- With no gutter (`set_gutter(false)`), the band still shows.
- Read sessions and verification through D7's events and breakpoints through D5's
  store API; `debug_session.rs` and `debug_breakpoints.rs` are not in scope.

## Notes on shared files

Mac T1806 (lent to neon) changes the same gutter, decoration and theme files (agent
lane marks), so this card waits for it. T1598, T1600, T1581 and T1817 also touch
theme files or `element/decorations.rs`; rebase over them.

## Not in this card

Conditional and logpoint glyphs and editing (D17), inline values (D18), commands
and keys (D8).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Mac** — acceptance includes a macOS look check of an audit scene (§5, §10), which review fails when made on Linux. The headless criteria can be built on Linux first.
