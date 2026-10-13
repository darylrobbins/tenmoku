---
task_uid: "01m4srvyrm2evt6nz04qw6rvx9"
task: "T620"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The decision file names, for each of the 8 fe-gpui scenes and workbench, the landed commit behind every changed region; text and caret move right by the gutter_width growth since 749a8728 and the line numbers don't move"
    required: true
    manual: false
  - id: "C2"
    text: "On the Mac, cargo run -p fe-gpui --example screenshots passes all 8 scenes"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac, cargo run -p fe-workbench --example screenshots passes"
    required: true
    manual: false
  - id: "C4"
    text: "Running both examples again with UPDATE_SCREENSHOTS=1 leaves every golden byte-identical"
    required: true
    manual: false
  - id: "C5"
    text: "Only the 9 golden PNGs and one sentence in TESTING.md's Screenshots section change; no file under crates/*/src changes"
    required: true
    manual: false
---

# Regenerate the fe-gpui and workbench screenshot goldens for the 1 em wider gutter from the change lane (147b9f1a, ef074cdf), after attributing every changed region

## Problem

The screenshot checks, which the gate doesn't run (TESTING.md "Screenshots"), fail on the Mac for all 8
fe-gpui scenes and the fe-workbench `workbench` scene. The pixel counts are the same on main c65979ea as on
stack 7, so the problem is in main's goldens and the stack didn't cause it. A run on 4c3a2cc2 on 2026-10-13
(`cargo run -p fe-gpui --example screenshots`) gave, for example, `tabs` 1.051%, `long_line` 2.010%,
`three_digit_gutter` 5.605% and `large_c` 3.454% of pixels different, against a 0.1% limit. In
`target/screenshots/tabs.diff.png`, every text glyph is drawn twice, about one character apart. The line
numbers don't move and no colours change.

The likely cause is two intended layout changes, not a font or macOS change. The fe-gpui goldens were last
regenerated in 749a8728 (T167, 2026-10-07) and the workbench golden in cc4e2d0a. Since then
`fe_display::layout::gutter_width` (`crates/fe-display/src/layout.rs`) has grown by 1 em, which moves
`text_left` right by 1 em while the line numbers stay where they were:

- 147b9f1a (neon T33, cherry-picked for Mac T484) added `CHANGE_GAP_EMS` (0.5) and `CHANGE_LANE_EMS` (0.5)
  to the width.
- ef074cdf (neon T66, cherry-picked for Mac T587) moved the lane next to the text. The gap is now
  `LINE_NUMBER_GAP_EMS`.

Both were built on Linux, where the scenes can't be compared, and cherry-picked. The Mac cards (T484, T587)
that would have regenerated the goldens were abandoned, so the goldens were never updated. Other commits
since 10-07 also touch painting (decorations, change colours b38a8093, kiln theme 801e1afb), so the 1 em
shift may not explain every changed pixel.

## Change

This runs on the Mac only. The scenes need Metal. Use only the two example commands in TESTING.md, and don't
loop them.

1. Before regenerating, explain the diffs. Run both examples once at base. For each scene, check that the
   text in `<scene>.actual.png` is the golden's text moved right by exactly the amount `gutter_width` grew
   since 749a8728, which should be 1 em at the scene font (Menlo). Check also that the line numbers stay in
   place and that the caret and selections move with the text. Record per scene, in the decision file,
   which landed commit explains each changed region.
2. If a region isn't explained by an intended change (a glyph metric difference, a colour, a clipped or
   missing element), it's a regression. Stop and report it with the scene and the region. Don't fold it into
   new goldens.
3. If every region is explained, regenerate with `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example
   screenshots` and `UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots`. Look at every
   changed PNG and commit them.
4. In TESTING.md's Screenshots section, add one sentence: a change to the gutter or text layout
   (`gutter_width`, `layout_frame*`) regenerates the goldens in the same change. A change made off the Mac
   needs a Mac follow-up to do that, as 147b9f1a and ef074cdf did in 2026-10.

## Implementation notes

Decision file: every changed region was attributed before regenerating. `gutter_width` grew by
`LINE_NUMBER_GAP_EMS + CHANGE_LANE_EMS` = 1 em since 749a8728 (Menlo 13 px: em = 7.83 px, 15.66 px at the
2x goldens). Measured against each golden, the best-fit horizontal shift of the text was 15.70–15.80 px.

- All 8 fe-gpui scenes (`highlighted_rust`, `multi_line_selection`, `multi_cursor`, `tabs`, `long_line`,
  `three_digit_gutter`, `markdown_fence`, `large_c`): text, carets and selection rectangles are the golden's
  moved right by 1 em, from 147b9f1a (adds CHANGE_GAP/CHANGE_LANE_EMS) and ef074cdf (moves the lane next
  to the text; gap is now LINE_NUMBER_GAP_EMS). Zero pixels differ left of text_left, so the line numbers
  don't move. Caret (122,162,247) and selection (41,57,86) are the same colours. Residual differences are
  glyph-edge antialiasing tints from the fractional shift. `long_line` and the right edge of `highlighted_rust`
  lose or gain a column at the clip edge for the same reason.
- `workbench`: the editor text moves right by 1 em for the same two commits (the last `;` of line 9
  clips at the pane edge). Line numbers and everything left of text_left, the tab bar, breadcrumbs, tree,
  and status bar are unchanged. One further region, x 1949-1978, y 301-330, is a fourth icon in the right
  activity rail: the Changes panel's dock icon from 990651e2 (Mac T376), after the golden's cc4e2d0a.

No unexplained region (no metric, colour, clipping or missing-element difference), so all 9 goldens were
regenerated. A second `UPDATE_SCREENSHOTS=1` run of both examples left them byte-identical; the plain runs pass.
