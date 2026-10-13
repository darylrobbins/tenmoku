---
task_uid: "01m4sd816xc89r748rxb1b6x6t"
task: "T66"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-display change_lane_beside_text_tests::order: for 500, 5000 and 50000 rows, scrolled to the top and the bottom, change_lane is Rect { x: text_left - 0.5 em, y: 0, width: 0.5 em, height }, fold_column.x + fold_column.width == change_lane.x, every line number ends 0.5 em left of fold_column.x, and gutter_width and wrap_width equal em × (digits + 2 + 0.5 + 0.5 + 3.75) and its wrap as before"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display change_lane_beside_text_tests::no_overlap: every fold toggle rect and chevron box lies inside fold_column, right of the line numbers and left of change_lane.x; a 3 px bar at change_lane.x ends at or before text_left and starts at least 0.75 em right of every chevron box"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-display -p fe-gpui passes with no fe-gpui file and no screenshot golden changed; in change_lane_tests.rs, fold_column_tests.rs and fold_gutter_tests.rs only the old-order position expectations and the CHANGE_GAP_EMS -\u003e LINE_NUMBER_GAP_EMS rename change; cargo clippy -p fe-display --all-targets -- -D warnings is clean"
    required: true
    manual: false
  - id: "C4"
    text: "git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Move the git change lane to just before the text, after the fold column (Mac T587)

Filed on argon from Mac T587 (card ~/coordination/argon-cards/T587.json rev 1). Mac task uid 01m4scswsbxrj6h93djg48905a. No dependencies (builds on Mac T484, long on main). Title the host commit "... (Mac T587)".

## Problem

The user, 2026-10-13: move the change bar next to the text. Their reference (a JetBrains editor) has, left to
right, the line numbers, the fold and gutter-icon area, then a thin change bar right before the text.

At main 5b11e1c4 the order is the other way round. T484 (147b9f1a) gave the change marks their own lane in
`crates/fe-display/src/layout.rs`: digits | `CHANGE_GAP_EMS` 0.5 em | `change_lane` 0.5 em | `fold_column`
3.75 em | text (`layout_frame_with_blocks`, `change_lane` at :624, `fold_column` at :1239, toggles at :1181,
line-number origins at :1130). The bar is about 3.75 em left of the text, with the chevrons in between.
`Decorations::layout` (`crates/fe-gpui/src/element/decorations.rs` :122) draws each bar at `frame.change_lane.x`
and the deletion mark across the lane, so moving the lane moves the marks. Nothing in fe-gpui needs to change.

## Change

Layout only, all in `crates/fe-display/src/layout.rs`:

- New order: digits | 0.5 em gap | fold column (3.75 em) | change lane (0.5 em) | text.
  - `change_lane.x = text_left - em * CHANGE_LANE_EMS`
  - `fold_column.x = text_left - em * (CHANGE_LANE_EMS + FOLD_COLUMN_EMS)`. Each `FoldToggle.rect` uses the same x.
  - Line numbers end `em * 0.5` left of `fold_column.x`.
- `gutter_width` keeps its sum, so `text_left`, `wrap_width` and the line numbers' x stay where they are. Only
  the fold column (with its chevrons) moves 0.5 em left, and the lane moves to the text's edge.
- Rename `CHANGE_GAP_EMS` to `LINE_NUMBER_GAP_EMS`, now the gap between the digits and the fold column. Update
  the doc comments of the constants, `gutter_width`, `Frame::change_lane` ("between the fold column and the
  text") and `Frame::fold_column`.
- The lane's width, the 3 px bar at its left edge (fe-gpui) and the chevron's 0.75 em padding don't change.
  That leaves 0.5 em minus 3 px between a bar and the text, and 0.75 em between a chevron and a bar.

The lane is the click target for the change bar: T547 (inbox) adds its hitbox from `frame.change_lane`, so it
follows the move with no change. The fold column's hitbox (`crates/fe-gpui/src/element.rs` :709) is
`frame.fold_column`, and it no longer touches the text.

Expectations that pin the old order are edited, and nothing else in those files changes:
`crates/fe-display/src/layout/change_lane_tests.rs` (delete `the_lane_sits_clear_of_the_numbers_and_the_chevrons`,
which the new file replaces, and rename the constant in `the_constants_keep_their_values`),
`crates/fe-display/src/layout/fold_column_tests.rs` (`column.x` 60 → 55 and its comment), and
`crates/fe-display/src/layout/fold_gutter_tests.rs` (the toggle x, and the "toggle ends at text_left"
assertion, which becomes "ends at change_lane.x").

No screenshot golden shows a chevron or a change bar (chevrons show only on hover or a fold, and no scene has a
diff base), so the goldens don't change. T584 owns them.

## Tests

New file `crates/fe-display/src/layout/change_lane_beside_text_tests.rs`, declared `#[cfg(test)]` from
layout.rs like `change_lane_tests`, using the fake monospace measurer (em 10).

After it lands, the lead updates the order described in T486 (human check) and T547.
