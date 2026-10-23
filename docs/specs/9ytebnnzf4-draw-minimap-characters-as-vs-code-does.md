---
task_uid: "01m5f7gymqjtkpwf9ytebnnzf4"
task: "T1036"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "With render_characters on, each non-whitespace printable ASCII char draws its baked mask (scale x 2*scale logical px) in its run's colour; other chars draw as a half-weight block; off draws card 2's blocks; toggling it rebuilds every tile and nothing else does (minimap_glyphs_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Masks exist for every printable ASCII char at scales 1-3 and scale factors 1 and 2; VS Code's MIT prebaked data is credited in NOTICE"
    required: true
    manual: false
  - id: "C3"
    text: "With side = left the minimap is at the editor's left edge and text clicks, change-bar clicks, fold chevrons and popover anchors still land right; switching side live re-lays the frame (view/minimap_left_tests.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "minimap_characters and minimap_left goldens committed and in TESTING.md; the minimap scene pins render_characters = false; workbench.png regenerated"
    required: true
    manual: false
---

# Draw minimap characters as VS Code does (editor.minimap.render_characters, the default) from baked glyph masks, and put the minimap on the left with editor.minimap.side

# Minimap 5/5: draw characters (editor.minimap.render_characters, the default) and put the minimap on the left

VS Code's minimap draws tiny glyph shapes by default (renderCharacters true) and solid blocks when off;
`side = "left"` puts it left of the gutter. Builds on cards 1-4. Mac-only (tile pixels, goldens).

## Characters

- With `editor.minimap.render_characters = true`, each non-whitespace char draws into card 3's tile as its own
  small alpha mask in the run's colour: `scale` x `2 * scale` logical px, so 2x4 device pixels at scale 1
  on a 2x display. Off, runs draw as card 2's blocks.
- Masks: gpui's glyph rasterizer is crate-private, so use baked sample data per printable ASCII char,
  adapted from VS Code's MIT prebaked minimap data (src/vs/editor/browser/viewParts/minimap/
  minimapPreBaked.ts; credit it in NOTICE as the vscode themes are) and resampled to each scale and
  scale factor; chars outside it (non-ASCII, wide chars over both their columns) draw as a block at
  VS Code's ~50% weight. Store the data in a new crates/fe-gpui/src/element/minimap_glyphs.rs.
- A change of this setting rebuilds every tile; nothing else does.

## Left side

`editor.minimap.side = "left"`: the minimap takes the editor's left edge, the gutter and text start after it, and
everything hit-tested in the text and gutter (clicks, the change bar, fold chevrons, hovers, IME,
completion and hover popover anchors) moves with them. The git lane (card 4) sits on its inner (right)
edge.

## Screenshots

Scenes `minimap_characters` (the default look) and `minimap_left` in
crates/fe-gpui/examples/screenshots/scenes.rs with goldens, listed in TESTING.md; card 2's `minimap` scene switches to
`render_characters = false` explicitly so its golden stays the block look. Regenerate
crates/fe-workbench/examples/screenshots/golden/workbench.png (characters are the workbench default).

## Tests (new files)

- crates/fe-gpui/src/element/minimap_glyphs_tests.rs: every printable ASCII char has a mask; masks at
  scales 1-3 and scale factors 1 and 2 have the right size; a space has none; a tile row of "ab" with
  characters on differs from blocks and matches a hand-computed pixel pattern.
- crates/fe-gpui/src/view/minimap_left_tests.rs: with the minimap left, a click at the first text
  column lands there, a click on the minimap scrolls and leaves the caret, a change-bar click still hits,
  switching the side live re-lays the frame.

## Implementation notes

- VS Code's data holds 96 samples (the printable ASCII chars and its unknown char) at 1 and 2 device
  pixels a column. Masks for 1 device pixel are the 1-pixel samples; 3, 4 and 6 (scale 2 and 3 at
  1x and 2x) are area-resampled from the 2-pixel ones. All are softened to 12/15 weight, as VS Code's
  normal font is. A column wider than 6 device pixels stretches the 6-pixel mask.
- The cached runs carry each run's glyph next to its colour (`Ink`). With characters on, each char
  that isn't whitespace is a run of its own, and runs of the same char merge. `render_characters` is
  part of the runs' key, so toggling it clears the runs and rebuilds every tile; the side and the
  slider aren't, so they rebuild none. The test-only `runs` that `painted_minimap` reports merge
  adjacent runs of one colour, as card 2 drew them.
- On the left, the frame's origin moves right by the minimap's width, so everything placed in frame
  coordinates (text, gutter, hitboxes, carets, popover and IME anchors) moves with it. The minimap
  sits at negative frame x, and its git lane is moved to the inner edge after `marks::layout`.
- `golden/minimap_marks.png` is regenerated too. The scene keeps the default options, so it now draws
  characters. Pinning blocks would have changed it as well, since blocks rows are 3 pixels tall
  rather than 2.
- When a frame's left edge moves (switching sides, or a left minimap's width changing), the element's
  prepaint hides the hover popover, shown or waiting, at the caret or the pointer, and the hover
  highlight, as a scroll does (`Hovered` records the scroll for that reason). Moving the anchor
  instead would mean changing hover.rs, which is outside this card's scope. The popover layer is built
  after the editor's prepaint, so no frame draws a popover at the old place. The next pointer rest or
  `editor.show_hover` finds it again (review round 1, F1).
