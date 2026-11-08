---
task_uid: "01m6vvs02n1pes36vskqvwe5bf"
task: "T1515"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test default_code_font_is_spline_sans_mono_with_20px_rows (cargo nextest run -p fe-gpui --test default_fonts) passes: with no editor.font_family the editor's font is Spline Sans Mono and a 13 px row is exactly 20 px"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::default_fonts::workbench_text_is_instrument_sans_13 (cargo nextest run -p fe-workbench) passes: the workbench root's text style is Instrument Sans at 13 px; hover, completion and signature help use the UI family"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-platform's capabilities test and the setup wizard font tests pass with Spline Sans Mono first in MONO_FAMILIES and the wizard's default"
    required: true
    manual: false
  - id: "C4"
    text: "On macOS, cargo nextest run -p fe-gpui --test non_latin_text passes (CJK and emoji still fall back)"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the same 28 goldens; every screenshot example pins Spline Sans Mono, not Menlo)"
    required: true
    manual: false
---

# Default to the design's fonts and type sizes: Instrument Sans UI at 13 px, Spline Sans Mono 13/20 code, Code small terminal; screenshot examples pin the bundled fonts

# Default to the design's fonts and type sizes

Spec: `docs/design/design-system.md#type-spacing-and-sizes`. Uses fe-fonts
(bundled by an earlier card) and kiln-ui's `roles.rs`.

## Change

- UI text: Tenmoku sets `kiln_ui::roles::FontFamilies` to Instrument Sans /
  Spline Sans Mono / Instrument Serif in `crates/fe-workbench/src/workbench.rs`
  (`Workbench::render`), and its root text drops `text_size(px(12.))` for
  `UI_BODY` (13/18). kiln's root (`kiln/crates/kiln-shell/src/root.rs`) reads
  the global's `ui` family and stays 13 px.
- Editor: the default `editor.font_family` resolves to Spline Sans Mono
  (`kiln/crates/kiln-platform/src/fonts.rs` `MONO_FAMILIES` first,
  `crates/fe-gpui/src/lib.rs` `font_family`), and the default
  `editor.line_height` gives 20 px rows at 13 px (`crates/fe-config/src/settings.rs`;
  rows must land on whole pixels so splits and diffs align).
  `crates/fe-gpui/src/element.rs:640`'s hard-coded `gpui::font("Menlo")` uses
  the code family. Terminal default: Code small (12/18).
- Popovers: `crates/fe-gpui/src/hover.rs` `UI_FONT` (`.SystemUIFont`) becomes
  the UI family, also used by `completion_menu.rs` and `signature_help.rs`.
- Setup wizard Font step: Spline Sans Mono is the first and default choice
  (`setup_wizard_preview.rs` `font_families`, `setup_wizard_model.rs`
  `default_font`); tests that pin `MONO_FAMILIES` / "Default (Menlo)" follow
  (`kiln/crates/kiln-platform/tests/capabilities.rs`,
  `crates/fe-workbench/src/setup_wizard_preview_tests.rs`).
- Every screenshot example registers fe-fonts and pins its code font to Spline
  Sans Mono instead of Menlo (`FONT_FAMILY` in fe-gpui, fe-workbench,
  fe-markdown-view and fe-chat-view example scenes), which also makes the
  goldens independent of installed fonts. Regenerate the same 28 goldens on
  the Mac.
- `crates/fe-gpui/tests/non_latin_text.rs` (macOS) relies on the editor font
  lacking CJK and emoji so CoreText falls back: check it still does with Spline
  Sans Mono.

Not here: widget heights (each surface card adopts its own), serif page titles
(the Settings card).

Platform: **Mac-only**.

## Lead ruling (rev 2)

The worker's scope-gap block (a full workspace nextest run, 8333/8345 passing) found these gaps:

1. **Line height 1.54:** tests that pin 1.6 follow (`crates/fe-config/src/tests/settings.rs`, `crates/fe-config/src/tests/layers.rs`). Scope widens to them.
2. **20 px rows:** test geometry follows the new row height (`crates/fe-gpui/src/scroll_off_tests.rs`, `crates/fe-gpui/src/view/mouse_binding_tests.rs`, `crates/fe-gpui/src/view/diagnostics_tests.rs`). These are test-only fixes and must keep each test's intent: measure from the row height rather than pinning a new constant where that's easy. Scope widens to them.
3. **Terminal: option (a).** The terminal default is Code small at 12 px, keeping `TerminalStyle::LINE_HEIGHT` 1.25 (15 px rows), so box-drawing rows still meet. The design's 18 px terminal rows wait until Tenmoku draws box-drawing glyphs itself, which is not this card. Record the departure in the decision file. Scope widens to `crates/fe-workbench/src/terminal_panel.rs` and `crates/fe-workbench/src/terminal_settings_tests.rs`. `fe-terminal-view` doesn't change.
4. **element.rs is dropped from this card.** The `gpui::font("Menlo")` at `element.rs:640` is in the `glyph_width_tests` module under a no-op text system, where the font name has no effect, so the "uses the code family" requirement above is withdrawn and `crates/fe-gpui/src/element.rs` leaves scope. T1497's carried finding F2 stays with the next card that changes element.rs.
