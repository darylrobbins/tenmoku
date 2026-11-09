---
task_uid: "01m6z6nmt3s1ag4kffp03rpa1m"
task: "T336"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "The fold-click branch of EditorView::press_proceeds (crates/fe-gpui/src/view.rs) no longer sets autoscroll or clear_band. A chevron or fold-marker click folds or unfolds as before and redraws, but the next frame keeps the view's scroll where it was. Keyboard, command and vim fold paths (fe-editor fold/unfold/fold_all/unfold_all, vim zc/zo/za/zM/zR) are unchanged."
    required: true
    manual: false
  - id: "C2"
    text: "New crates/fe-gpui/src/tests/fold_chevron_scroll.rs, declared by a single mod line in crates/fe-gpui/src/tests.rs and using base APIs only: tests::fold_chevron_scroll::collapsing_by_its_chevron_keeps_the_view_where_it_is and tests::fold_chevron_scroll::expanding_by_its_chevron_keeps_the_view_where_it_is (the view scrolled to row 100 of a 240-row Rust file with the caret at offset 0; after a click on row 108's chevron, scroll_top and the chevron's position are unchanged) fail at base and pass with the fix."
    required: true
    manual: false
  - id: "C3"
    text: "tests::fold_chevron_scroll::collapsing_over_the_caret_moves_it_to_the_header_in_place passes: a caret on row 109 moves to the end of row 108 and scroll_top is unchanged. fold_gutter_tests (clicking_a_chevron_toggles_its_fold, clicking_the_marker_unfolds, clicking_keeps_the_selections_a_fold_doesnt_hide) pass unchanged."
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui passes, and cargo clippy -p fe-gpui --all-targets -- -D warnings is clean."
    required: true
    manual: false
---

# Clicking a fold chevron or fold marker no longer scrolls the editor to the caret (it jumped to the top of the file): the view and the fold's header stay where they were (Mac T1648)

## Bug

User report: "When collapsing a region, the editor jumps to the top rather than
staying where we just collapsed." It happens when collapsing with the gutter's
fold chevron (a mouse click on the fold toggle).

Repro: open a long source file (the caret stays at the top of the file, row 0).
Scroll down with the wheel or trackpad, which leaves the caret where it is.
Click a fold chevron in the gutter. The fold collapses and the view jumps back
to the top of the file, to the caret. Expanding a fold with its chevron, or by
clicking the fold marker after a folded row, jumps the same way. If the caret is
somewhere else off screen, the view jumps there instead.

## Root cause

`EditorView::press_proceeds` in `crates/fe-gpui/src/view.rs:1146-1148` treats a
fold click like a caret move:

```rust
if self.toggle_fold_at(event.position, cx) {
    self.drag = None;
    self.changed(cx);
    return;
}
```

`changed()` (view.rs:871) sets `autoscroll = true` and `clear_band = true`. On
the next frame:

- `fe_display::layout` (crates/fe-display/src/layout.rs:927-936) scrolls the
  primary caret into view, so a caret above the viewport pulls `scroll.y` up to
  it. A caret at offset 0 pulls it to 0.
- Even with `autoscroll` off, `clear_band` makes element.rs (1038-1053, through
  `sticky_scroll::clear_of_caret`) scroll toward a caret that sits above the
  view whenever sticky scroll's band pins anything.

A fold click does not move the caret unless the fold hides it, and in that case
`move_folded_carets` moves it to the fold's header, which is the row that was
just clicked and is already on screen. So the click has nothing to scroll to.

### Other entry points

- The fold marker after a folded row (`fold_click_areas` covers chevrons and
  markers) goes through the same `toggle_fold_at` branch, so it jumps too.
- In vim mode the click takes the same branch (through `split_session`), so it
  jumps too.
- The keyboard paths do not show this bug: the editor commands `fold`,
  `unfold`, `fold_all` and `unfold_all` (fe-editor commands.rs:509-540,
  `Effect::Changed`) and vim's zc/zo/za/zM/zR (fe-vim folds.rs). Each acts at
  the caret, so scrolling the caret into view afterwards is correct there.
  Leave them unchanged.

## Fix

In the fold-click branch of `press_proceeds` in
`crates/fe-gpui/src/view.rs`, do not call `self.changed(cx)`. Do what it does
apart from scrolling:

- `band_spares_caret = false`
- `sticky_reveal = None`
- `blurred_ghost_text = None`
- `sync_find(cx)` if a fold change needs it (the text does not change, so it
  probably does not)
- `cx.notify()`

Leave `autoscroll` and `clear_band` unset (false). Alternatively, call
`changed()` and then reset `autoscroll` and `clear_band` to false. That gives
the same result. Pick whichever reads better and give the reason in a comment.

`scroll.y` is in pixels, and a fold only hides rows below its header. So the
clicked header, and every row above it, keep their screen y once nothing
autoscrolls. No anchor remapping is needed.

Do not change `toggle_fold_at`'s fold and unfold logic. Do not change
`EditorModel::fold_rows` or `unfold_rows`, or any keyboard, command or vim fold
path.

## Expected behaviour

- After collapsing with a chevron, the fold's header row stays at the same
  screen y, and the view's scroll top stays where it was (VS Code and JetBrains
  behave this way).
- A caret outside the fold stays where it is, even off screen, and the view
  does not follow it.
- A caret inside the folded rows moves to the end of the header (existing
  behaviour, `fold_gutter_tests::clicking_keeps_the_selections_a_fold_doesnt_hide`),
  and the view does not scroll.
- Expanding with the chevron or the fold marker likewise leaves the header row
  and the scroll top where they were.

## Regression tests

Add a new file, `crates/fe-gpui/src/tests/fold_chevron_scroll.rs`. Declare it
from `crates/fe-gpui/src/tests.rs` with a single line, `mod fold_chevron_scroll;`,
next to `mod vim_visual_keys;`. That `mod` line must be the whole of
`tests.rs`'s diff. The file must compile at base and use only base APIs:
`EditorView::set_scroll_top`, `EditorView::scroll_top`, `view.layout` with
`frame.fold_toggles` and `layout.bounds(rect).center()` (as in
`crates/fe-gpui/src/fold_gutter_tests.rs`), `model_mut().fold_rows`,
`cx.simulate_click`, and `cx.run_until_parked`. Assert with `assert_eq!`, and
give `assert!` no custom message, so a failure at base reads as an assertion.

Setup:

- A focused Rust editor (`Some("main.rs".into())`) on 60 functions of 4 rows
  each, `"fn fN() {\n    a();\n    b();\n}\n"`, which is 240 rows.
- The caret stays at offset 0. Wait until the tree is parsed, as
  `fold_gutter_tests::editor` does.
- `set_scroll_top(100.)`, then run until parked so the frame is drawn. Row 100
  is a function header, so sticky scroll's band is empty.
- Read `scroll_top()` from the view and the centre of row 108's chevron from the
  frame.

Tests:

1. `collapsing_by_its_chevron_keeps_the_view_where_it_is`. Click row 108's
   unfolded chevron. Then the folds are `[(108, 110)]`. `scroll_top()` equals
   the value read before the click. Row 108's chevron centre is unchanged. The
   caret is still at offset 0. At base `scroll_top()` becomes 0, so the test
   fails.
2. `expanding_by_its_chevron_keeps_the_view_where_it_is`. Before scrolling,
   fold with `model_mut().fold_rows(108..=110)`. Click row 108's folded
   chevron. Then there are no folds, and `scroll_top()` and the chevron centre
   are unchanged. At base the view jumps to 0, so the test fails.
3. This one is not a regression test, and it passes at base too:
   `collapsing_over_the_caret_moves_it_to_the_header_in_place`. Put the caret
   at the start of row 109 and click row 108's chevron. Then the caret is at
   the end of row 108 and `scroll_top()` is unchanged.

## Out of scope

- Scroll anchoring when Fold All or Unfold All (keyboard, command or vim
  zM/zR) collapses or expands rows above the viewport. There `scroll.y` stays
  in pixels and the caret is scrolled into view, which is a separate behaviour.
  If it needs work, raise it as a finding rather than fixing it here.
- The diff view's fold toggles (`diff_fold.rs`).
- Gutter visuals. T1549 (ready) reworks the chevrons in element.rs to show only
  on hover. This card does not touch element.rs.
