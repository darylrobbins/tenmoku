---
task_uid: "01m4kcax7et1zbh3wp6zz41qj3"
task: "T1"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "view::find_tests::peer_edit_tests::find_next_and_previous_after_a_peer_edit_select_the_match: on \"a foo b foo\" with foo set and the caret at 0, a peer (model().split()) inserts \"xx \" at 0 with no frame drawn; find_next then selects 5..8 (the text there is foo), find_previous from there selects the last foo in the new text, and find_count is None until the recount lands, then has current at the selected match"
    required: true
    manual: false
  - id: "C2"
    text: "view::find_tests::peer_edit_tests::find_from_after_a_peer_edit_selects_the_match: after the same peer insert with no frame, find_from(0) selects 5..8, and the text there is foo"
    required: true
    manual: false
  - id: "C3"
    text: "find_tests.rs gains only the line #[cfg(test)] mod peer_edit_tests; every existing fe-gpui test passes unchanged"
    required: true
    manual: false
---

# Catch up with shared-buffer edits before find_next, find_previous and find_from search, so a peer edit can't misplace the selected match

## Problem

T264's review (2026-10-10, codex, F4 medium; filed in T318): `EditorView::find_from` and `step_find`
(`crates/fe-gpui/src/view/find.rs:120,132`) search `self.model.text()` and read `primary_selection()` from the
model's cached snapshot. Nothing catches the model up first with edits made through another view of the shared
buffer; only prepaint does that (`element.rs:440`, `model.sync_display()`). If a peer edits before the next
frame, the search runs on the old text. `select_and_reveal` then catches up through `unfold_rows` and applies
those old offsets to the new text, so it selects unrelated characters. The match count isn't brought up to date
either.

## Change

At the start of `find_from` and `step_find`, before reading the pattern's text or the selection, call
`self.model.sync_display()` and then `self.sync_find(cx)`, the same catch-up prepaint does. No other change.

## Tests (new file)

`crates/fe-gpui/src/view/find_tests/peer_edit_tests.rs`, declared from `crates/fe-gpui/src/view/find_tests.rs`
as `#[cfg(test)] mod peer_edit_tests;` (commit that line). Use `super::*` helpers and make the peer with
`model().split()`, as `count_off_thread` does. Invoke navigation without drawing a frame in between.


_Moved from the Mac board (T319) to run on neon; it lands back on main through the lead._
