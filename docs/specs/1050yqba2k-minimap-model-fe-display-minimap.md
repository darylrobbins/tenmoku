---
task_uid: "01m5fbn0gm16h9mn1050yqba2k"
task: "T207"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "[editor.minimap] parses enabled, side, render_characters, max_column, scale, show_slider and size with VS Code's defaults (true, right, true, 120, 1, mouseover, proportional); scale outside 1..=3 is a diagnostic; template and schema golden updated and their tests pass"
    required: true
    manual: false
  - id: "C2"
    text: "All six bundled themes set the 11 minimap_* [editor] keys with the spec's VS Code values, EditorTheme carries them, and a user theme's override takes (minimap_theme_keys.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "fe_display::minimap geometry: width = max_column*scale capped for narrow editors and hidden under 2 columns; proportional/fill/fit row heights and minimap top at the file's top, middle and end; slider top/height; click centres the row; dragging the slider to the bottom reaches the last page (minimap_tests.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "row_runs splits at whitespace and colour changes in display columns (tabs expanded, wide chars two), merges, clips at max_column and fills unspanned bytes with the default colour"
    required: true
    manual: false
  - id: "C5"
    text: "fe-display stays GPUI-free; cargo test -p fe-display -p fe-config passes on Linux"
    required: true
    manual: false
---

# Minimap model: fe_display::minimap geometry (scale, max column, proportional/fill/fit, slider, click and drag mapping) and row runs, the [editor.minimap] settings and the minimap theme colours (Mac T1032)

# Minimap 1/5: the minimap's geometry and row runs (fe-display), its settings and theme colours (fe-config, EditorTheme)

The user asked for VS Code's editor minimap (new feature; nothing exists yet). This card is the headless,
GPUI-free half that the paint cards (2-5) build on. Linux-OK: everything here is testable without a window.

## Settings (`[editor.minimap]` table; VS Code's names and defaults)

A `MinimapSettings` struct as `EditorSettings::minimap` (crates/fe-config/src/settings.rs), a nested
`[editor.minimap]` table like the sticky-scroll drafts' `[editor.sticky_scroll]`, so VS Code's
`editor.minimap.*` names carry over. In the template (its own commented `[editor.minimap]` section; teach
the template test about nested editor tables if sticky scroll hasn't already) and the schema golden
(`UPDATE_GOLDEN=1`), each documented like its neighbours:

| key | type | default |
|---|---|---|
| `enabled` | bool | true |
| `side` | `"right"` \| `"left"` | right |
| `render_characters` | bool | true |
| `max_column` | int, min 1 | 120 |
| `scale` | int 1..=3 | 1 |
| `show_slider` | `"mouseover"` \| `"always"` | mouseover |
| `size` | `"proportional"` \| `"fill"` \| `"fit"` | proportional |

Enums are snake_case serde enums as `AutoSave` is.

## Theme colours (`[editor]` table, `EditorColors`)

New keys appended at the end of `colors!` (sticky scroll's go after `indent_guide_active`), with values in all six bundled themes (crates/fe-config/themes/*.toml), VS Code's defaults for
the appearance (dark / light), in the vscode-* themes VS Code's own:

- `minimap_background` = the theme's `background` (minimap.background defaults to the editor's)
- `minimap_slider` #79797933 / #64646433, `minimap_slider_hover` #64646459 / #64646459,
  `minimap_slider_active` #BFBFBF33 / #0000004D (minimapSlider.*: scrollbarSlider.* at half alpha)
- `minimap_selection` #264F78 / #ADD6FF, `minimap_find_match` #D18616 / #D18616
- `minimap_error` #FF1212B3 / #FF1212B3, `minimap_warning` = the theme's `warning`
- `minimap_gutter_added` / `_modified` / `_deleted` = the theme's `change_added` / `_modified` / `_deleted`

Add the same fields to `EditorTheme` (crates/fe-gpui/src/lib.rs) and fill them in
`EditorTheme::colored` (crates/fe-gpui/src/theme.rs). Nothing paints them yet.

## Geometry: `fe_display::minimap` (new module, no GPUI)

Pure functions over plain numbers, all in logical pixels, mirroring VS Code's layout on a 2x display:

- `MinimapOptions { side, scale, max_column, size, show_slider, render_characters }` with its own enums
  (fe-display can't use fe-config).
- A row is `2 * scale` px tall, a column `scale` px wide. Width = `max_column * scale`, but a narrow
  editor shrinks it as VS Code does: at most `floor(text_width * col_w / (em + col_w))`; under 2 columns
  it is hidden (width 0). The text area is what's left.
- `MinimapLayout::new(options, editor_height, display_rows, first_visible_display_row (fractional),
  visible_display_rows)` gives: the minimap's x range for `side`, the row height actually used, the
  first display row it draws and its y, and the slider's top and height.
  - `proportional`: row height fixed; if all rows fit, row 0 is at the top and nothing scrolls;
    otherwise the minimap scrolls with the editor as VS Code's does:
    `minimap_top = editor_fraction * (content_px - minimap_px)` so the slider reaches both ends together.
  - `fill`: the row height that makes all rows fill the height (stretch or shrink), never scrolling.
  - `fit`: as proportional when all rows fit at the proportional height, else shrunk as `fill`.
  - Slider: covers the visible rows; at least one row tall.
- `scroll_for_click(y) -> display row to centre` (VS Code reveals the clicked row in the centre) and
  `scroll_for_drag(start_first_row, dy) -> first row`, mapping slider travel to editor travel so the
  slider follows the pointer exactly.

## Row runs: `fe_display::minimap::row_runs`

`row_runs<C: Copy + PartialEq>(row: &str, tab_size, spans: &[(Range<usize>, C)], default: C,
max_column) -> Vec<MinimapRun<C>>` where `MinimapRun { columns: Range<u32>, color: C }`: the row's
non-whitespace runs in display columns (tabs expanded as `ExpandedRow` does, a wide char two columns),
split where the colour changes and at whitespace, clipped at `max_column`, adjacent same-colour runs
with no gap merged. Spans are byte ranges within the row (as `styled_highlights` hands them out); bytes
with no span take `default`.

## Tests (new files)

- crates/fe-display/src/minimap_tests.rs: widths (scale, max_column, narrow shrink, hidden), each size
  mode's row height and minimap top at top / middle / end of a long file and a short one, slider top and
  height, click centring, drag mapping round-trips (dragging the slider to the bottom reaches the last
  page), runs (tabs, wide chars, whitespace splits, merges, clip, default colour).
- crates/fe-config/tests/minimap_settings.rs: defaults, parsing every enum value, out-of-range scale
  rejected with a diagnostic.
- crates/fe-config/tests/minimap_theme_keys.rs: every bundled theme has every new key; light and dark
  values as above; a user theme overriding `minimap_slider` takes.

## Not in this card

Painting, mouse handling, workbench wiring (card 2); caching and tiles (3); decorations (4); glyph
rendering and side left (5).

## Implementation notes

- `MinimapLayout::new(options, &MinimapViewport { width, height, em, rows, first_visible_row,
  visible_rows })`: the viewport struct carries the editor's width and em too, which the x range
  and the narrow-editor cap need.
- The editor scrolls until its last row is at the top (fe-display's layout clamps there), so, as
  VS Code does with `scrollBeyondLastLine`, the minimap lays out the rows plus the space scrolling
  past the end reaches: `rows - 1 + visible_rows`. "All rows fit" counts that space, `fill`
  divides the height by it, and `proportional`'s `content_px` is it at the row height. That keeps
  the slider exactly over the visible rows in every mode, the slider and the minimap reach their
  ends together, and a drag maps `dy` to rows at one constant rate, so the slider follows the
  pointer exactly. `fill` is never taller than the editor's rows (VS Code caps it at the line
  height); with that content count it never needs to be.
- Rows are `3 * scale` tall with `render_characters = false`, as VS Code's blocks are
  (`baseCharHeight` 3); `2 * scale` otherwise.
- Wide chars are VS Code's `isFullWidthCharacter` ranges, plus astral chars (two UTF-16 units, two
  columns in VS Code's minimap). Tab stops count each char as one column, as `ExpandedRow` and VS
  Code's `_renderLine` do. fe-display gains no dependency.
- Theme colours: the vscode-2026 themes take their JSON's own `minimapSlider.*` and
  `editor.selectionBackground` (which `minimap.selectionHighlight` defaults to); every other
  minimap key is the spec's default for the appearance. `minimap_find_match` is the spec's opaque
  #D18616 in all six themes. VS Code's own `minimap.findMatchHighlight` defaults to
  `editor.findMatchHighlightBackground`, #EA5C0055, which would barely show on a one-row mark. The
  theme headers say so.
- Carried T200 F1: Dark Modern's `error` and `information` are now VS Code's editor defaults,
  #F14C4C and #59A4F9, and its header explains them. The check is
  `minimap_theme_keys.rs::dark_modern_diagnostics_match_upstream`, because
  `vscode_dark_modern_theme.rs` is outside this card's scope.
