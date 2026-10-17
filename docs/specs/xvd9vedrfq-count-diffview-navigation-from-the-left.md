---
task_uid: "01m4ykvv8j86cnj4xvd9vedrfq"
task: "T720"
rev: 4
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_view::diff_view_inline_tests::inline_navigation_tests::left_next_change_after_inline_counts_from_its_own_caret: with three hunks (the first below row 0), next_change twice inline, then side by side and focus on the untouched left editor: next_change puts the left caret on the first hunk's left start row, not the third's"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test diff_view::diff_view_inline_tests::inline_navigation_tests::left_previous_change_after_inline_counts_from_its_own_caret, same setup: previous_change from the left editor wraps to the last hunk with the wrapped-to-last message, not to the first hunk"
    required: true
    manual: false
  - id: "C3"
    text: "Right-side navigation across a toggle is unchanged: next_change side by side to hunk 2, toggle inline, then next_change goes to hunk 3; existing diff_view_tests and diff_view_inline_tests pass unchanged"
    required: true
    manual: false
---

# Test DiffView navigation from the left caret after inline mode, which helium's git/diff chain already fixed at base (T720 tests only)

## Problem

Mac T546 (inline DiffView, landed as helium T118 in host stack 9, e26ce867) was approved by codex (review 1n54pm on
candidate m9q3ze) with two medium findings open. F2 is covered by Mac T660. F3 is covered by no card (helium batch 52
entry in ~/coordination/landing.md; `.worktrees/gate-stack-9/LANDING-stack-9.md`, "Open review findings carried in"):

> Inline navigation records a fictitious left caret at offset 0 in visited, and switching modes retains that record.
> Navigate inline to the second hunk, toggle back, focus the untouched left editor whose caret is still at offset 0,
> and next_change incorrectly continues from the second hunk instead of navigating to the first change after the
> left caret; previous_change is likewise affected.

On Mac main e4d2b586, `crates/fe-gpui/src/diff_view.rs` `DiffView::go_to_change` records
`self.visited = Some((index, carets))` with `carets[0] = 0` when `self.inline` (the left side isn't shown or moved),
and `crates/fe-gpui/src/diff_view/inline.rs` `DiffView::set_inline` keeps `visited` across the switch. Side by side
again, the left editor's real caret at offset 0 equals the recorded 0, so the `carets[focused] == head` arms count
from the inline hunk instead of `TextDiff::next_hunk` / `prev_hunk` from the left caret's row.

## Change

In `crates/fe-gpui/src/diff_view.rs`, `visited` becomes `Option<(usize, [Option<usize>; 2])>`: a side's entry is the
caret navigation put there, `None` for a side it didn't place (the left side while inline). `go_to_change` records
`None` for the left side inline, and the visited arms match only `carets[focused] == Some(head)`. The right side's
record survives a mode switch as now, so its end-of-side behaviour (a hunk starting past the last row) is unchanged.
Update the field's doc comment. `set_inline` needs no change.

## Tests

New `crates/fe-gpui/src/diff_view_inline_navigation_tests.rs`, declared from the EXISTING
`crates/fe-gpui/src/diff_view_inline_tests.rs` (add `#[path = "diff_view_inline_navigation_tests.rs"] mod
inline_navigation_tests;` there), not from diff_view.rs: Regatta's base overlay copies only *_tests.rs files, so a
mod line in diff_view.rs would leave the module missing at base and the regression check would run no tests. The
test ids are therefore `diff_view::diff_view_inline_tests::inline_navigation_tests::<fn>`. Texts with three hunks, the first starting below row 0 on both sides.
