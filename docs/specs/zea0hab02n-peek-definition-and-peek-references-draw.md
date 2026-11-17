---
task_uid: "01m7nexcw5c1cdy4zea0hab02n"
task: "T677"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "peek::peek_frame_tests::peek_frame_is_a_one_pixel_hairline_in_the_border_colour passes: with a peek open, the block's top and bottom edges are 1 px and drawn in ui.border, not ui.accent, in the Ember and Monokai themes"
    required: true
    manual: false
  - id: "C2"
    text: "peek::peek_frame_tests::peek_references_frame_is_the_same_hairline passes: the Peek References block draws the same 1 px ui.border frame"
    required: true
    manual: false
  - id: "C3"
    text: "peek::peek_frame_tests::excerpt_height_follows_the_one_pixel_frame passes: the reserved rows still hold the header plus the excerpt, with no overflow or gap from the thinner frame"
    required: true
    manual: false
  - id: "C4"
    text: "The existing peek_tests, peek_references_tests and peek_robustness_tests pass, updated (not deleted) where they pinned the 2 px frame or the accent colour"
    required: true
    manual: false
---

# Peek Definition and Peek References: draw the frame as a 1 px neutral hairline (ui.border) instead of a 2 px accent border, in every theme (Mac T1929)

## Problem

The peek block's top and bottom edges are 2 px of `ui.accent` (crates/fe-workbench/src/peek.rs,
render: `.border_t(BORDER).border_b(BORDER).border_color(ui.accent)`, `BORDER = px(2.)`). On the
tenmoku.dev hero they read as stray orange lines between panes. User decision 11-17: a neutral
hairline. Peek References reuses the same block, so one change covers both.

## Change

- Set `BORDER` to `px(1.)` and the frame colour to `ui.border` (the token the peek header and its
  dividers already use). Update the doc comment ("accent borders" -> "hairline borders").
- The excerpt height maths (`HEADER_HEIGHT + BORDER * 2.`) follows the thinner frame.
- No new strings, settings or theme tokens.

## Pins

Peek tests that assume the 2 px frame or the accent colour (mainly reserved-row geometry in
peek_tests.rs) are updated, not deleted. New tests go in the new
crates/fe-workbench/src/peek_frame_tests.rs, declared from peek.rs next to `mod peek_tests;`.
No audit scene or golden shows an open peek; confirm at finish. The hero recapture is not in scope.
