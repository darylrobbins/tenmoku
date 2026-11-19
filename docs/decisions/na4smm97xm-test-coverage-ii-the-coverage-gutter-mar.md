# Test coverage II: the coverage gutter mark and its colours (T701, cov2)

The renderer half of test coverage: `GutterMark::Coverage(CoverageMark)`
(`Covered | Uncovered | Partial`, `crates/fe-editor/src/decorations.rs`)
drawn by `crates/fe-gpui/src/element/decorations.rs`, and three `[editor]`
theme keys. cov3 turns a coverage report into these decorations.

## Where the mark goes

- **In the change lane's gap, no new column.** The lane is one em
  (`CHANGE_LANE_EMS`): the git bar takes `GUTTER_BAR` (3) of its 13 design
  pixels at the left, and the other 10 are the gap before the code. The strip
  is `GUTTER_COVERAGE` (2) of those 13, scaled with the font and rounded to
  device pixels as the bar is, so the gutter keeps its 81 px at 13 px code and
  no `fe-display` width or golden changes.
- **Right of the bar, `BAND_EDGE` (2 px) short of the code**, not flush with
  it: the debugger's current-line and caller-frame bands draw a 2 px edge in
  the last of the gap (`Decorations::band_edges`), and a strip flush with the
  code would be painted over on those rows. Its left edge is clamped to the
  bar's right edge, so it never overlaps the bar at any size. Measured in the
  headless test (scale factor 2): at 13 px code the bar is x 68–71, the strip
  77–79 and the code starts at 81; at 20 px the bar is 105–109.5, the strip
  120–123 and the code 125. The gap is wide enough at every default size; no
  overlap with the lane's left half was needed.
- **One strip per display row**, inset 1 px top and bottom as a bar's run is
  (`BAR_INSET`), so adjacent marks of different kinds stay apart, and every
  piece of a soft-wrapped row gets its own.
- **Partial is a shape, not only a colour**: the same strip as two stacked
  dashes with a one-strip-wide gap, spanning what one strip would, so it reads
  without colour (the design's colour-blind notes).
- **Not touched by the agent lane**: the agent's dots replace the bar on its
  rows (`clear_bars_from_agent_runs` only trims `GutterMark::Bar`), and sit at
  the lane's left; the strip keeps its place beside them. The tint is unused:
  the theme's colour for the mark draws it, at full strength (the bars are
  faded to 0.6; the strip is thinner and must hold 3:1 by itself).
- Test API: `DrawnGutter::coverage`, each drawn piece's bounds, mark and colour.

## Colours

`coverage_covered`, `coverage_uncovered`, `coverage_partial`, declared after
`agent_row` and before the debugger's and the minimap's keys.

- **Covered is a teal, not celadon.** The design's celadon means added *and*
  passed, but the strip is drawn right beside the git bar, so a covered row in
  the added green would read as an added row. Yohen and Hakuji take seiji
  (`#7fc9be` / `#0d6e66`, the type colour); the other themes their own teal or
  cyan (Catppuccin's teal, VS Code's type teal, Monokai's cyan, One Dark's
  cyan for Tenmoku Dark and Light, a sage teal for Ember).
- **Uncovered is shinsha** (failed; the theme's red), **partial kiseto**
  (flaky; the theme's warning colour), each theme's own. Catppuccin Latte's
  peach (`#FE640B`, 2.6:1) is darkened to `#D35A00` and VS Code 2026 Light's
  warning yellow (`#B69500`, 2.9:1) to `#A07D00` to reach 3:1.
- **Every bundled theme spells them out**, all 13, not only the six that spell
  out every key: with a fallback to the added colour (the card's first
  suggestion) the other seven would draw covered in the git green.
- **Fallbacks** (`COVERAGE_FALLBACKS`, as `DEBUG_FALLBACKS`) for a user theme
  with an `[editor]` table that leaves them out: covered its `information`
  colour, uncovered its `change_deleted`, partial its `warning`. Covered falls
  back to information rather than added, for the reason above.
- Every value is opaque and at least 3:1 on the editor background in every
  bundled theme, the three differ, none is the theme's modified colour, and
  covered is never its added colour (`crates/fe-config/tests/coverage_colors.rs`).
- Colour-blind mode leaves them as they are: `[color_blind.editor]` swaps only
  the added/removed roles, and `color_blind.rs` pins that table's keys. The
  partial mark's shape and the teal-versus-red pair are what separate them;
  swapping uncovered to the colour-blind orange is left to a follow-up.
