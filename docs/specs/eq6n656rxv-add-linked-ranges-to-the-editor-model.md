---
task_uid: "01m5n9hzb3wdsqcxeq6n656rxv"
task: "T213"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "EditorModel::set_linked_ranges/clear_linked_ranges/linked_ranges exist; typing, backspace, delete-word, paste and cut inside one range are copied to every other range at the same inner offset in one undo step, and all ranges follow the edit"
    required: true
    manual: false
  - id: "C2"
    text: "The session ends on an edit outside or crossing a range, the selection leaving the linked ranges, a second selection, a non-word character in the edited range, an undo past the edit or a new set/clear; a snippet session takes precedence"
    required: true
    manual: false
  - id: "C3"
    text: "New tests in linked_ranges_tests.rs including a proptest that mirrored ranges stay equal pass headless; existing snippet and typing tests pass unchanged"
    required: true
    manual: false
---

# Add linked ranges to the editor model: typing in one range is mirrored into the others in the same undo step, ending when an edit leaves them (Mac T1110)

## Problem

Groundwork for linked editing (VS Code's `editor.linkedEditing`, e.g. renaming an HTML opening tag renames its closing tag as you type). The headless editor model has one mirroring mechanism, the snippet session's (crates/fe-editor/src/snippet.rs: "typing in one occurrence of the stop visited is copied into its mirrors, in the same undo step", wired in crates/fe-editor/src/lib.rs at `mirror_snippet_edits` :1450 and `sync_snippet_mirrors` :1481), and nothing a host can drive with ranges of its own. This card adds that to fe-editor with no LSP and no GPUI, so a later card can feed it a language server's `textDocument/linkedEditingRange`.

## Change

New `crates/fe-editor/src/linked_ranges.rs` (`pub mod linked_ranges`; a `linked: Option<LinkedSession>` field on `EditorModel` in lib.rs, hooked at the same points as `snippet`: `follow_snippet` calls at :384, :805, :1405 and the edit path at :1450-:1488):

- `EditorModel::set_linked_ranges(&mut self, ranges: Vec<Range<usize>>)` (bytes of the current text; fewer than 2, overlapping or out-of-bounds ranges, or a model with several selections, clear instead), `clear_linked_ranges()`, `linked_ranges() -> Option<&[Range<usize>]>`.
- While active and the single selection (caret or selection) is inside one range (touching its ends counts as inside): an insert or delete that lies wholly inside that range is copied into every other range at the same offset within the range (so ranges of unequal text still work), in the same undo step as the edit (as snippet mirrors are), and all ranges follow the edit (grow when typing at their ends, shift when an edit lies before them). Typing, backspace, delete-word, paste and cut inside a range all mirror.
- The session ends (ranges dropped, nothing mirrored) when: an edit falls outside every range or crosses a range's boundary; the selection leaves the range it was in (moves into another linked range keeps it); a second selection is added; the edited range's new text has a character that is not alphanumeric, `_`, `-`, `.` or `:` (the server's `wordPattern` is ignored in this version; documented in the module doc); `set_linked_ranges`/`clear_linked_ranges` is called; an undo or redo past the edit.
- A snippet session wins: while one is active `set_linked_ranges` does nothing.
- Vim: the same edits made by vim insert mode mirror too, as they go through the same edit path (no vim code change).

## Tests

New `crates/fe-editor/src/linked_ranges_tests.rs` (#[path] mod at the end of linked_ranges.rs): `typing_in_one_range_mirrors_to_the_other`, `backspace_and_delete_word_mirror`, `paste_mirrors`, `unequal_ranges_mirror_at_the_same_inner_offset`, `one_undo_step_undoes_both`, `an_edit_before_the_ranges_shifts_them`, `an_edit_outside_ends_the_session`, `an_edit_crossing_a_boundary_ends_the_session`, `leaving_the_range_ends_it_and_moving_to_the_other_keeps_it`, `a_non_word_char_ends_the_session_unmirrored`, `a_second_cursor_ends_it`, `a_snippet_session_wins`, plus a `proptest` (dev-dependency already present) that random typing inside one range keeps all ranges' texts equal. Linux-OK (headless model, no GPUI).
