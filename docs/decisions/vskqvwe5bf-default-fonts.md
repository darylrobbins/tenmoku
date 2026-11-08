# The design's fonts and type sizes as Tenmoku's defaults (T1515)

Tenmoku now uses the design's three families by default
(`docs/design/design-system.md#type-spacing-and-sizes`):

- **UI.** `fe_workbench::init` registers the bundled fonts and sets
  `kiln_ui::FontFamilies` to Instrument Sans, Spline Sans Mono and Instrument
  Serif. Kiln's `Root` sets its text in the global's `ui` family at 13 px.
  Tenmoku's windows don't wrap the workbench in `Root`, so the workbench's own
  root sets the UI family too. Its overlays, the setup sheet and the toasts,
  therefore inherit it as well (review round 1, F2). The workspace's content
  draws at `UI_BODY` (13/18) instead of 12 px. The editor's hover, completion
  and signature help popovers set their prose in `fe_gpui::ui_font_family`,
  which is the same family. The containers that hold a host renderer's
  Markdown (the hover body, the completion documentation) set it as well, so
  the workbench's compact renderer, which takes its prose font from the text
  style it's drawn in, doesn't inherit the completion menu's code font (round
  1, F1). Without the global, the popovers and `Root` keep the platform's UI
  font.
- **Editor.** Spline Sans Mono is now first in `MONO_FAMILIES`. An unset
  `editor.font_family` takes the host's code family if it is installed, and
  otherwise the first installed entry of that list. The default
  `editor.line_height` is 1.54: rows round to whole pixels, so 13 px text gets
  20 px rows (before, 1.6 gave 21 px).
- **Setup wizard.** Spline Sans Mono is listed first and is the Font step's
  default, because the wizard reads `MONO_FAMILIES`.
- **Screenshot examples.** All four examples (fe-gpui, fe-workbench,
  fe-markdown-view, fe-chat-view) register fe-fonts and use Spline Sans Mono
  for code, so the goldens no longer depend on Menlo. Their UI text stays in
  the platform UI font, except fe-workbench's, which `fe_workbench::init`
  switches to Instrument Sans.

## Where it departs from the spec

- **Terminal: Code small at 12 px, but 15 px rows, not 18 (lead ruling, option
  a).** An unset `terminal.font_size` is now 12 px. Before, it was the editor's
  size. `TerminalStyle::LINE_HEIGHT` stays 1.25, giving 15 px rows. The terminal
  takes box-drawing characters from the font, and at 1.5 (18 px) the borders of
  full-screen programs would leave gaps. The design's 18 px rows wait until
  Tenmoku draws box-drawing characters itself.
- **Families set in `fe_workbench::init`, not in `Workbench::render`.** That is
  where the bundled fonts are registered, so the families can't name a font
  that isn't there. Setting a global from a render call would also override a
  host's own choice on every frame.
- **`element.rs` is out of the card (lead ruling).** The `gpui::font("Menlo")`
  there is in `glyph_width_tests`, which runs under the no-op text system, so
  the font name has no effect. T1497's carried finding F2 (element.rs:3334)
  stays with the next card that changes `element.rs`.

## Follow-ups made here

- **Tests that pinned the old geometry.** The fe-config tests that pinned 1.6
  now expect 1.54. Three fe-gpui tests depended on 21 px rows. Each keeps what
  it tested:
  - `scroll_off_tests::sticky_margin_follows_the_option` computes the centred
    row from the frame's row height.
  - `mouse_binding_tests::hover_underlines_the_word_under_cmd` measures the
    word again after scrolling.
  - `diagnostics_tests::row_end_hover_and_changed_diagnostics` rests the
    pointer past the row end between hovers, so the previous popover no longer
    covers the next row.
- **T1513 F1.** `TerminalPanel` now observes `ActivePalette`. A theme file's
  reload or a system appearance change, with no settings reload, now restyles
  the running sessions and the style that new sessions start with: the ANSI
  colours, the `[terminal]` overrides, and the editor colours they fall back to.
  The regression test is
  `terminal_settings_tests::a_palette_change_restyles_the_sessions`.
- **CJK and emoji.** Spline Sans Mono has none of these glyphs, so CoreText
  still takes them from other fonts.
  `fe-gpui --test default_fonts cjk_and_emoji_fall_back_from_spline_sans_mono`
  checks this. `non_latin_text` passes unchanged.
