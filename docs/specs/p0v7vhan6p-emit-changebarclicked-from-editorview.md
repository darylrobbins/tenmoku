---
task_uid: "01m4rhrmkvyp56s3p0v7vhan6p"
task: "T547"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::change_bar_click_tests::emits_rows: with a diff base making rows 4-6 added, a click in the change lane on row 5 emits ChangeBarClicked { rows: 4..6 } once and puts the caret at row 4 column 0; on a deletion mark, the empty range at its row"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::change_bar_click_tests::elsewhere_unchanged: a click on the change lane of an unchanged row, on the line numbers and on the fold chevrons behaves as before (no event; fold toggles on a chevron)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Emit ChangeBarClicked from EditorView when the git change bar is clicked, with the change's rows

## Problem

The user (2026-10-12) asked to open a file's diff, and stage or revert a hunk, from the gutter. The git change bar
(T484 gives it its own lane) takes no clicks: `EditorView`'s `on_mouse_down` (`crates/fe-gpui/src/view.rs` :504)
handles the fold column only.

## Change

- New file `crates/fe-gpui/src/view/change_bar.rs` (declared from `view.rs`): a click on the change lane T484 lays out
  (since T587, ef074cdf: the gutter strip after the fold column, just before the text), on a row with a `Gutter(Bar)` or `Gutter(Deletion)`
  decoration from the row-change provider, emits `EditorViewEvent::ChangeBarClicked { rows: Range<u32> }` (the
  `RowChange`'s rows; for a deletion the empty range at its row) and moves the caret to the change's first row. The
  pointer is a hand over a bar. A click on the lane without a change does what a gutter click did before.
- `crates/fe-gpui/src/element.rs`: a hitbox for the change lane, like `fold_column` (:52, :676).
- `EditorViewEvent` is the view-seams card's enum; this adds the variant.

Not in this card: what the host does on the event (the diff wiring card opens the diff at that change).

## Tests

New file `crates/fe-gpui/src/view/change_bar_click_tests.rs` (declared `#[cfg(test)]` from `view.rs`).
