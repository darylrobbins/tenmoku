---
task_uid: "01m417z4pphww6e8kk9j6gzgre"
task: "T31"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-display test layout::fold_gutter_tests::toggles_for_candidates_and_folds: a toggle sits in the gutter's right margin of each visible row heading a candidate or a fold (folded flagged), and none on wrapped continuation rows"
    required: true
    manual: false
  - id: "C2"
    text: "fe-display test layout::fold_gutter_tests::markers_have_hit_rects: each fold marker's rect covers the painted marker"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui tests fold_gutter_tests::clicking_a_chevron_toggles_its_fold and fold_gutter_tests::clicking_keeps_the_selections_a_fold_doesnt_hide: a click folds the candidate and a second click unfolds it; carets and selections outside the fold don't move, and a caret the fold hides moves to the end of the fold's header row, as keyboard folds do (user decision 2026-10-04, T31 review F1)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test fold_gutter_tests::clicking_the_marker_unfolds: a click on the marker unfolds the row and the caret doesn't move"
    required: true
    manual: false
  - id: "C5"
    text: "Manual: a person checks the chevrons' look and the pointer cursor (agents can't take screenshots until T-06)"
    required: true
    manual: false
---

# Show fold chevrons in the gutter and toggle folds by clicking them or the fold marker

Paddock T-25, part 2 of 2: fold chevrons in the gutter, and toggling folds by clicking (P3, draft:
needs the user's approval before `rgt task ready`). Depends on T-25a (`fe_syntax::fold_ranges`,
`EditorModel::fold_rows` / `unfold_rows`).

## Problem

Folds can be made and removed only with the keyboard. Nothing shows which rows can fold. The ⋯ marker
after a folded row (`Frame::fold_markers`, `crates/fe-display/src/layout.rs`; painted in
`crates/fe-gpui/src/element.rs`) does nothing when clicked. A click on it places the caret
(`EditorView::on_mouse_down`, `crates/fe-gpui/src/view.rs`).

## Change

- **Layout** (`crates/fe-display/src/layout.rs`): `layout_frame` takes a fold-candidate callback,
  `FnMut(Range<u32>) -> Vec<u32>`, returning the buffer rows in a range that head a candidate. It is called
  once with the visible buffer rows. `Frame` gains `fold_toggles: Vec<FoldToggle { rect: Rect,
  buffer_row: u32, folded: bool }>`, one per visible row that is the first display row of a buffer row and
  either heads a candidate or heads a fold (`DisplayRow::folded`). Each is placed in the gutter's right
  margin, between the line number and the text (the `em * 2` gap). Fold markers get hit rectangles, so
  `fold_markers` carries a `Rect`. Tests go in a new
  `crates/fe-display/src/layout/fold_gutter_tests.rs`, declared from `layout.rs`.
- **Paint** (`crates/fe-gpui/src/element.rs`): pass `fe_syntax::fold_ranges` over the view's syntax (when
  its version matches) as the callback. Paint each toggle as a chevron in the gutter colour, pointing right
  when folded and down when not, shaped from a glyph like the ⋯ marker.
- **Clicks** (`crates/fe-gpui/src/view.rs`, `on_mouse_down`): before placing the caret, hit-test the last
  frame's toggles and markers. A folded toggle or a marker unfolds that row (`unfold_rows`). An unfolded
  toggle folds its candidate (`fold_rows`). The caret and selection don't move, and no drag starts. Show
  `CursorStyle::PointingHand` over toggles and markers.
- GPUI tests go in a new `crates/fe-gpui/src/fold_gutter_tests.rs`, declared in `crates/fe-gpui/src/lib.rs`.
  Click at positions taken from the frame's `fold_toggles` and `fold_markers`.

Agents can't take screenshots until T-06 lands, so how the chevrons look is checked by a person.

## Open questions

- Should chevrons always show, as here, or only while the pointer is over the gutter (VS Code's default)?

## Implementation notes

- **Chevrons** are `›` (folded) and `⌄` (unfolded). JetBrains Mono and Menlo both have them. If they
  look mismatched in C5, `▸` / `▾` are in both fonts too. The swap is two constants in `element.rs`.
- **Always shown**, as the spec says. Showing them only on hover (the open question) is left for later.
- **Plain text shows no chevrons.** Candidates come only from the syntax tree, but keyboard
  fold-all falls back to indented blocks. A row folded by the keyboard still gets a folded chevron.
- **Folded flag:** a toggle is folded when the buffer row after it is hidden
  (`DisplayMap::hidden_rows`), not from `DisplayRow::folded`. With soft wrap on, that flag is set on
  the header's *last* display row. The chevron sits on the first, and the last may be out of view.
- **Unfolded toggle** folds `fold_range_at(row)` only when that candidate starts at the row, which is
  the same candidate the keyboard Fold picks.
- **Click areas** (`Frame::fold_click_areas`) are what clicks and the pointing-hand hitboxes both use:
  toggles clipped to the editor, markers clipped to the text area. So a marker half under the gutter
  takes clicks only on its visible half (review F2).
- **Hidden carets move (review F1; user decision 2026-10-04, now in C3).** This overrides the
  Change section's "the caret and selection don't move". A click doesn't place the caret or start a
  drag, and carets and selections outside the folded rows stay as they are, reversed or not. A caret
  *in* the rows the fold hides moves to the end of the fold's header row, as `fold_rows` does for
  keyboard folds and as VS Code does. Leaving it there would put a caret where it can't be seen, and
  typing would edit hidden text. C3's tests `fold_gutter_tests::clicking_a_chevron_toggles_its_fold` and
  `fold_gutter_tests::clicking_keeps_the_selections_a_fold_doesnt_hide` pin this down.
- **Cost:** the candidate callback is called once per frame over the first to last visible buffer
  rows. When a fold hides many rows, that range, and the tree walk, include the hidden ones. Calling it
  once per run of consecutive visible rows would bound this if it ever shows up in frame timings.
