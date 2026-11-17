---
task_uid: "01m6w0atk43z659pgsy1t9jnq0"
task: "T1600"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-gpui --test styled_spans pass: an_unstyled_row_shapes_as_one_run, a_styled_row_has_one_run_per_style_change, every_face_has_the_same_advance, a_boundary_inside_a_grapheme_cluster_does_not_split_it"
    required: true
    manual: false
  - id: "C2"
    text: "The bench shaping 1000 highlighted Rust rows with and without styles was run (cargo bench -p fe-gpui --bench styled_shape) and its result recorded in PERF.md: styled within 15% of unstyled, else the per-style font ids and run lists are cached per row before merging"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_GOLDEN=1 in this change and looked at (fe-gpui's goldens and workbench.png)"
    required: true
    manual: false
---

# Draw italic and bold syntax spans: style runs in the shaper, one run when nothing is styled, a bench in PERF.md

# Draw italic and bold syntax spans

Spec: Syntax (italic comments, bold definitions). fe-gpui shapes a row once in
a single font and applies colour as decoration runs
(`TextMeasure for Shaper` in `crates/fe-gpui/src/element.rs`); a span style
that changes the face has to change the shaping.

## Change

- Carry a `SpanStyle { color, italic, bold }` through the foreground spans
  and `StyleRun` consumers in fe-gpui (the `Hsla` style type becomes
  `SpanStyle` where highlights flow: `element.rs`, `element/decorations.rs`,
  `element/sticky_scroll.rs`, `file_viewer/element.rs`); `Shaper::shape`
  builds one `TextRun` per style run with the italic/bold variant of the code
  font (the faces from the bundling card). Column advance is unchanged: all
  faces are monospaced at the same advance, and a test asserts it.
- A row with no styled span shapes exactly as today (one run), so the common
  case costs nothing. Bold and italic spans in a ligature font do not break
  ligatures across a style boundary any more than colour runs do.
- Performance: a bench `crates/fe-gpui/benches/styled_shape.rs` shapes 1,000
  rows of highlighted Rust with and without styles; the result is recorded in
  `PERF.md` and styled must stay within 15% of unstyled. If it does not, cache
  the per-style `Font` ids and the run lists per row before merging.
- Uses the theme data from the font-style data card; Yohen and Hakuji
  therefore show italic comments. Goldens that contain comments are regenerated.

## Not in this card

Bold at definitions only (the next card supplies the span), closing-brace
hints in italic (a one-line follow-up there).

Platform: **Mac-only** (goldens).

## Implementation notes

- **Types.** `fe_gpui::SpanStyle { color, italic, bold }` is the text area's
  style (`FrameInput<SpanStyle>`), sticky scroll's and the file viewer's.
  `styled_highlights`, `over`, `clipped` and `fade_spans` are generic over a
  private `HighlightStyle` (implemented for `SpanStyle` and `Hsla`), so the
  minimap, which draws no faces and is outside this card, keeps calling them
  with `Hsla` unchanged; `foreground_spans` stays the `Hsla` one and
  `styled_foreground_spans` gives faces. `Shaper` keeps `Style = Hsla` for the
  plain text it shapes elsewhere (lens rows, hints, ghost text, proposals,
  inline diagnostics, line numbers); `StyledShaper` wraps it for the rows.
- **Runs.** `fe_gpui::text_runs` builds the `TextRun`s from
  `fe_gpui::Faces` (the font and its italic, bold and bold italic, resolved
  once a row first draws one). Colour stays in decoration runs, so a row is
  cut only where its face changes; with no styled span it is one run, the
  path before this card. Boundaries move to grapheme-cluster ends: a cluster
  is drawn in the face of the run it starts in. Bold is weight 600
  (SemiBold, the bundled family's heaviest; CSS matching takes a Bold where a
  family has no SemiBold). A face whose advance (of `m`) differs from the
  font's own is replaced by the font's own, so a configured family with a
  wider italic keeps its columns.
- **Ligatures.** GPUI's macOS shaper already breaks ligatures at every font
  run boundary (alternate runs get a size one ulp larger), so a ligature can
  only break where the face changes, never at a colour change.
- **Performance (C2).** A run of spaces alone takes the face before it, or
  the one after it at the start of a row. Without that, the indented doc
  comments that make up nearly all styled rows cost 56% more to shape (two
  CoreText runs), and the bench came out at +14.4%. With it they are one run:
  +0.1% to +0.6% for all rows, +0.4% for styled rows alone (PERF.md, "Italic
  and bold spans"). The card's fallback (caching font ids and run lists per
  row) would not have helped: the cost is CoreText shaping each run, and GPUI
  resolves a run's font by a hash lookup. The bench times `text_runs` and
  `layout_line`; the decoration runs and bidi check it leaves out cost the
  same either way.
- **Font fix (scope added by respec).** `every_face_has_the_same_advance`
  found that T1599's `SplineSansMono-Italic.ttf` (the 400 instance) has `a`,
  `aacute`, `abreve`, `acircumflex`, `adieresis`, `agrave`, `amacron`,
  `aogonek`, `aring`, `aringacute` and `atilde` at 1196 units, where every
  other one-cell glyph of every face is 1200; each italic `a` pulled its row
  0.03 px left at 13 px. Their advances are set to 1200, outlines unchanged;
  two-cell ligature glyphs (2400) and zero-width marks keep theirs. Recorded
  in `crates/fe-fonts/fonts/README.md` and THIRD_PARTY.md (OFL-1.1 allows
  modified versions; no Reserved Font Name). Reproduce from T1599's file:

  ```sh
  git show 406478f44:crates/fe-fonts/fonts/spline-sans-mono/SplineSansMono-Italic.ttf > SplineSansMono-Italic.ttf
  uvx --from fonttools==4.60.1 python -I - SplineSansMono-Italic.ttf 1200 <<'PY'
  import sys
  from fontTools.ttLib import TTFont
  path, cell = sys.argv[1], int(sys.argv[2])
  font = TTFont(path, recalcTimestamp=False)
  metrics = font["hmtx"].metrics
  for name, (advance, lsb) in metrics.items():
      if advance != cell and abs(advance - cell) <= cell // 100:
          metrics[name] = (cell, lsb)
  font.save(path)
  PY
  shasum -a 256 SplineSansMono-Italic.ttf  # 3883d4704b6dd1ac510db414e657aa251c27e3b5da2187659817fcdcd98e4f08
  ```

  A `ttx` diff of the two files shows only the 11 `<mtx>` widths and
  `checkSumAdjustment`.
- **Tests.** Beyond C1's four, `the_editor_draws_comments_in_the_italic_face`
  draws an `EditorView` with CoreText and checks the comment's glyphs are in
  the italic face of the theme's font (with its features) and the code's in
  the plain one, via the new `EditorView::drawn_font_id`.
- **Known limits.** Soft wrap and long-row slicing measure clusters in the
  plain face (`GlyphWidths`); with equal advances that is exact for the code
  family's glyphs, but a fallback font CoreText picks for a char the italic
  lacks (CJK in a comment) may differ slightly in width from the plain
  face's fallback. Carets and hit-testing follow the shaped glyphs, so they
  stay on what's drawn.
- **Goldens (C3).** The harness's update variable is `UPDATE_SCREENSHOTS=1`
  (`kiln_platform::screenshots::goldens::UPDATE`), not `UPDATE_GOLDEN`. fe-gpui:
  the 12 scenes that show a comment changed, each only in its comments' pixels
  (checked in every diff image); the other 5 were rewritten byte-identical.
  fe-workbench: `workbench.png` (the `///` line) and, added to the scope by
  respec, `setup_wizard_dark.png` and `setup_wizard_light.png`, whose theme
  preview has a `//` comment (Yohen and Hakuji, both italic). The update also
  rewrote `file_icons_dark/light.png` with sub-tolerance noise; those were
  restored, and every scene of both examples matches its golden.
