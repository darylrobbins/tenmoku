---
task_uid: "01m4tehzx8ve71nafaf2xwqk8w"
task: "T109"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-buffer test selection_clipping::carets_clipped_to_one_offset_are_one_selection: SelectionSet::from_resolved of carets 5..5 and 9..9 in abc has len 1 and equals SelectionSet::caret at 3; carets 0..0 and 1..1 in é give len 1"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test selection_clipping::range_clipped_to_empty_is_a_caret: in aé, from_resolved of 2..3 resolves the same as caret(3) after inserting X at offset 3"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test selection_clipping::ranges_inside_the_text_keep_their_shape: from_resolved of 4..6 and 1..3 in abcdef keeps 2 selections, resolving to [1..3, 5..7] after inserting ! at 6 and ? at 4"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-buffer/src/buffer.rs and proposed.rs are unchanged; the existing fe-buffer tests pass unchanged"
    required: true
    manual: false
---

# Clip selections to the text before merging them in fe-buffer's SelectionSet::from_resolved, so carets that clip together are one selection and a range clipped to empty is a caret (Mac T643)

## Problem

`SelectionSet::from_resolved` (crates/fe-buffer/src/selection.rs) merges the given ranges with `merge` on
their raw offsets, and only then turns each into anchors with `snapshot.anchor_at`, which clips the offset
to the text and onto a char boundary (end of text; floor for `Bias::Left`, ceil for `Bias::Right`). It also
decides caret versus non-empty selection on the unclipped range. Two results break the type's own contract
("kept sorted and non-overlapping"; "carets stay before text inserted at them"):

- Two carets that clip to the same offset stay two selections. `len()` and `iter()` report the duplicate;
  only `resolve()` hides it by merging again. fe-editor counts and iterates the raw set
  (`multi_cursor.rs` `selections.len() > 1`, `lib.rs` `selections.iter()`).
- A non-empty range inside one multibyte char clips to an empty selection whose start is `Bias::Right`, so
  text inserted at it lands before it, unlike a caret.

Checked 2026-10-13 at Mac e6988aba (selection.rs is identical on helium main df738a8):
`from_resolved(&Buffer::new("abc").snapshot(), [5..5, 9..9])` has `len() == 2` and is not equal to
`SelectionSet::caret(&snap, 3)`; in "é", carets 0 and 1 both floor to 0 and stay two. In "aé",
`from_resolved([2..3])` resolves to 4..4 after inserting "X" at 3, where `caret(3)` resolves to 3..3.

## Change

In crates/fe-buffer/src/selection.rs `from_resolved`: clip every range against `snapshot.text()` before
`merge`, with the directions the anchors use today (a non-empty range's start towards `Bias::Right`, its
end towards `Bias::Left`; a caret `Bias::Left`). A range that clips to empty becomes a caret (both anchors
`Bias::Left`, one shared anchor as for a caret today). Then merge and build anchors as now. `merge`,
`resolve` and `refresh` are unchanged; buffer.rs and proposed.rs are out of scope.

## Tests

New file crates/fe-buffer/tests/selection_clipping.rs (public API only):

- `carets_clipped_to_one_offset_are_one_selection` (regression, fails at base): in "abc",
  `from_resolved([5..5, 9..9])` has `len() == 1` and equals `caret(&snap, 3)`; in "é",
  `from_resolved([0..0, 1..1])` has `len() == 1`.
- `range_clipped_to_empty_is_a_caret` (regression, fails at base): in "aé", `from_resolved([2..3])` and
  `caret(3)` resolve to the same selections after `edit([(3..3, "X")], Origin::User)`.
- `ranges_inside_the_text_keep_their_shape` (control): in "abcdef", `from_resolved([4..6, 1..3])` has 2
  selections, and after inserting "!" at 6 and "?" at 4 they resolve to [1..3, 5..7].
