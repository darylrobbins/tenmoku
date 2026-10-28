---
task_uid: "01m5xmbad8tz7m8hsba8r8wvd8"
task: "T1203"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "primary_identity_tests::cmd_d_makes_the_added_match_primary: carets on two words, cmd-d (select_next_occurrence) with the match landing above the earlier selections after a wrap: primary_selection() is the added match, not the bottommost, and selections() still has all of them"
    required: true
    manual: false
  - id: "C2"
    text: "primary_identity_tests::cmd_d_searches_from_the_primary: after that wrapped cmd-d, the next cmd-d searches from the primary's end (the new match), not the bottommost selection's, and skips selections it overlaps"
    required: true
    manual: false
  - id: "C3"
    text: "primary_identity_tests::move_makes_the_moved_selection_primary: move_last_selection_to_next_find_match replaces the primary selection (not the bottommost) with the next match, wrapping included, keeps the others, and the moved selection is primary; two calls in a row move that same selection on"
    required: true
    manual: false
  - id: "C4"
    text: "primary_identity_tests::identity_clears_on_other_changes: after a cmd-d, set_cursor, select_range, extend_primary_to, a plain set_selections and Escape each leave primary_selection() = the last (or, for Escape, the tracked primary alone, then cleared) as before; typing at the carets keeps the tracked primary on the same selection; an edit that merges it into another makes the merged one primary"
    required: true
    manual: false
  - id: "C5"
    text: "primary_identity_tests::cursor_undo_restores_identity: cmd-d and cmd-k cmd-d are each one Cursor Undo step, no text-undo entry; cursor_undo restores the previous selections and their primary, cursor_redo the moved one with its primary"
    required: true
    manual: false
  - id: "C6"
    text: "selection_primary::merge_and_fallback: SelectionSet from_resolved_with_primary keeps the primary through sorting, follows it when merged into an overlapping selection, and primary_in falls back to the last selection when none is tracked"
    required: true
    manual: false
  - id: "C7"
    text: "primary_reveal_tests::wrapped_cmd_d_scrolls_to_the_match: in a headless window with a document taller than the viewport, scrolled to the bottom with a selection on a word, cmd-d whose next match wraps to the top scrolls the view so the new match's row is visible; same for cmd-k cmd-d; without a wrap the existing follow still works"
    required: true
    manual: false
  - id: "C8"
    text: "Existing tests pass unchanged: fe-editor multi_cursor_tests, move_to_match_tests, cursor_history_tests, fe-gpui multi_cursor_tests and keymap_tests, fe-keymap move_to_match_key and vim_multi_cursor_keys, and the fe-vim suites that read primary_selection"
    required: true
    manual: false
---

# Track the primary selection's identity for cmd-d and cmd-k cmd-d and reveal it, wrap included (follow-up to T1186)

Follow-up to T1186 (landed, 0dc9014b). Its rev 2/3 documented two limits: the primary selection is positional (the bottommost), so after Add Selection to Next Find Match (select_next_occurrence, cmd-d) or Move Last Selection to Next Find Match (move_last_selection_to_next_find_match, cmd-k cmd-d) the primary is not the selection the command just added or moved; and when the match is found by wrapping to above the other selections the view does not scroll to it. Fix both together, as VS Code does: the selection a cmd-d or cmd-k cmd-d most recently added or moved is the primary, and the view reveals it.

## Where things live (verified)
- `EditorModel::primary_selection` (crates/fe-editor/src/lib.rs ~820) is `self.selections().pop()`: the last of the position-sorted set. `EditorModel::set_selections` (lib.rs ~834) builds a `SelectionSet` (crates/fe-buffer/src/selection.rs: sorted `Vec<Selection>` of anchors; `from_resolved`, `resolve`, `refresh`, `merge`), records Cursor Undo, and stores it. Nothing in the set says which selection is primary.
- cmd-d: `EditorModel::select_next_occurrence` and `needle`, `seed_carets` in crates/fe-editor/src/multi_cursor.rs. It searches from `selections.last().range.end` and takes the needle from `primary_selection()`. cmd-k cmd-d: crates/fe-editor/src/move_to_match.rs, same shape (`selections.pop()` then push).
- Reveal: no new view code is needed if the primary is right. crates/fe-gpui/src/element.rs builds `FrameInput.primary_head` from `model.primary_selection().head()` (a search preview overrides it); `EditorView::changed` (view.rs ~706) sets `autoscroll = true` after any `Effect::Changed`. So a wrapped match is revealed once `primary_selection()` returns it.
- Cursor Undo (crates/fe-editor/src/cursor_history.rs) stores whole `SelectionSet` clones, so identity stored in the set is restored with it. `record_cursor_history` compares the primary's row via `selections.last()`: leave that positional unless it breaks a test (it only decides coalescing).

## Change
1. Give `SelectionSet` an optional primary: one selection's anchors (or its index kept consistent through `merge`). `SelectionSet::resolve` is unchanged. Add `SelectionSet::from_resolved_with_primary` (or equivalent) and `SelectionSet::primary_in(&BufferSnapshot) -> Option<ResolvedSelection>`; if the primary merged into another selection, the merged one is primary; if it no longer exists, fall back to the last.
2. `EditorModel::primary_selection` returns the tracked primary when set, else the last selection (today's behaviour). Add `EditorModel::set_selections_with_primary(selections, primary: Range<usize>)` (pub(crate)); plain `set_selections` and every other path that replaces the set (`set_cursor`, `select_range`, `extend_primary_to`, `set_selections_moving`, column select, escape/single_selection) clears the tracked primary. Typing/edits keep it through the anchors.
3. `select_next_occurrence` (match added) and `move_last_selection_to_next_find_match` (match moved) set the new match as primary. Both search from the primary's end, not the bottommost's, and replace/keep "the primary" accordingly: cmd-k cmd-d replaces the primary selection (not `pop()` of the bottommost). `seed_carets` leaves the primary as it was (positional unless already tracked).
4. Remove the "documented limit" wording from move_to_match.rs docs and from the select_next_occurrence doc comment; keep the doc comments accurate.

## Must not change
- With no tracked primary (every path other than the two commands) `primary_selection()` is the bottommost, exactly as today. All other readers (vim's marks/visual/edit/cmdline/insert, completion, hover, signature help, IME, ghost text, indent guides, pane_item state, column_select, scripted_ai, backup) therefore see no difference. Vim is off for both bindings (context `editor && !vim.enabled`); identity is cleared by any set_selections.
- Cmd-d / cmd-k cmd-d still pick the same matches as T1186 and are one Cursor Undo step with no text-undo entry. Select All Occurrences, Escape (keeps only the primary: now the tracked one, if any), Cursor Undo/Redo semantics, multi-cursor typing and undo unchanged.

## Out of scope
Settings, keymap, command catalog (no new command; cmd-d and cmd-k cmd-d ids and bindings stay), fe-gpui view code (change it only if the reveal turns out not to follow; say so in a review note).

## Tests (new files; register with `#[cfg(test)] #[path = ...] mod` in lib.rs / view.rs)
- crates/fe-editor/src/primary_identity_tests.rs (headless EditorModel, dispatch like case_transform_tests.rs)
- crates/fe-gpui/src/view/primary_reveal_tests.rs (headless window, like multi_cursor_tests.rs)
- crates/fe-buffer/tests/selection_primary.rs (SelectionSet)

## Implementation notes

- `SelectionSet` keeps the tracked primary as an index into its sorted selections (`primary: Option<usize>`), carried through sorting and merging by `merge_tracking`; `resolve` is unchanged, `primary_in` and `primary_index_in` resolve with the index, and `refresh` keeps it.
- Edits: `apply_through` (and `apply_row_edits` in rows.rs, added to the scope by respec) anchor the selections an edit leaves through `EditorModel::carry_primary`. With one selection per old selection (typing, deleting), the tracked primary keeps its index; when the edit merged overlapping edits (fewer selections, e.g. delete-word-left over carets in one word), the primary is the first new selection touching where the old primary's anchors went. After a text undo or redo the primary is the last again. Carets the edit makes coincide merge, and the merged one is primary.
- Cursor Undo/Redo restore the stepped-to set's primary (`step_cursor_history` rebuilds the set from its resolved selections with the same index). Both `record_cursor_history` and `step_cursor_history` count a change of the effective primary (`primary_in`) as a change even when the ranges are equal (review round 1, F2); coalescing still compares rows by the last selection.
- `move_to_match_tests::wraps_and_skips` asserted the documented limit (the primary of `foo [bar] [foo]` after cmd-k cmd-d was `bar`); it now asserts the moved match, `0..3`. No other existing test changed.
- No fe-gpui view code changed: the view follows `primary_selection()` already, which `primary_reveal_tests` shows for a wrapped cmd-d and cmd-k cmd-d.
