---
task_uid: "01m47p69tsedwfxv0vwksj3nby"
task: "T94"
rev: 3
type: "bugfix"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-editor test tests::batch_structure::insert_then_delete_through_one_borrow_keeps_the_fold: insert \"X\" at 6, then delete 6..7, through one extracted \u0026mut Buffer; b catches up, its text equals the original, and b.display().folds() == [Fold{start:1,end:3}]. Each test uses \"top\\ndef a():\\n    one\\n    two\\nend\" with rows 1..=3 folded in sibling b and, except the scaling test, checks catching up both through sync_display and by dropping b.buffer_mut()."
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test tests::batch_structure::transient_header_split_through_one_borrow_unfolds: insert \"\\n\" at 6, then delete 6..7, through one extracted \u0026mut Buffer; b's folds are [], as syncing after each edit gives (guards against keeping too much). Each test uses \"top\\ndef a():\\n    one\\n    two\\nend\" with rows 1..=3 folded in sibling b and, except the scaling test, checks catching up both through sync_display and by dropping b.buffer_mut()."
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test tests::batch_structure::grouped_overlapping_replacements_keep_the_fold_through_undo_and_redo: one transaction replaces 6..7 with \"X\", then 6..7 with \"Y\"; b keeps fold 1..=3 after the edit, after a.undo() (text equals the original) and after a.redo() (text \"top\\ndeY a():\\n    one\\n    two\\nend\"). Each test uses \"top\\ndef a():\\n    one\\n    two\\nend\" with rows 1..=3 folded in sibling b and, except the scaling test, checks catching up both through sync_display and by dropping b.buffer_mut()."
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::batch_structure::grouped_insertions_through_one_borrow_keep_the_fold_through_undo_and_redo: T90's case, \"X\" at 6 and \"Y\" at 7 in one transaction through one extracted \u0026mut Buffer; b keeps fold 1..=3 after the edit, after undo and after redo (text \"top\\ndeXYf a():\\n    one\\n    two\\nend\"). Each test uses \"top\\ndef a():\\n    one\\n    two\\nend\" with rows 1..=3 folded in sibling b and, except the scaling test, checks catching up both through sync_display and by dropping b.buffer_mut()."
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test tests::batch_structure::many_batches_recorded_together_take_linear_work: 10,000 one-byte appends at EOF through one extracted \u0026mut Buffer; the span from the borrow through dropping the guard and b.sync_display() takes under 2 s (base: 4.0-8.5 s in debug, of which the edits take 34 ms), b keeps fold 1..=3, and b's text length is the original plus 10,000. A time bound because base has no work counter and a new one would not compile at base."
    required: true
    manual: false
  - id: "C6"
    text: "History::record no longer reconstructs texts: Known, Pieced, Piece, Source and splice are gone from crates/fe-editor/src/lib.rs, and BufferEvent::Edited carries the text after its batch"
    required: true
    manual: false
  - id: "C7"
    text: "Every existing fe-editor tests::shared_buffer test passes with crates/fe-editor/src/tests/shared_buffer.rs unchanged, and all fe-buffer tests pass with events_describe_each_batch changed only by adding .."
    required: true
    manual: false
---

# Capture each batch's structure when it is applied, not by replaying batches afterwards

# Capture each batch's structure when it is applied, not by replaying batches afterwards

Redesign, decided by the user on 2026-10-06 after four consecutive follow-ups on this code (T87 → T90 → T93 →
T94): stop reconstructing batch structure after the fact and patching the next gap.

## Problem

Since T86, fe-editor works out each pending batch's structure only when it records: `History::record` in
crates/fe-editor/src/lib.rs (~1191) rebuilds the texts between batches. It does this with `Known::new`,
`splice` and `Pieced`, starting from the text before and the text after. Two problems remain (T93's review,
F1 and F2):

- **Folds dropped.** Bytes one batch inserts and a later batch removes are unknown. With
  "top\ndef a():\n    one\n    two\nend" and rows 1..=3 folded in a sibling, two cases lose the fold, both
  through `sync_display` and through dropping `BufferMut`:
  - inserting "X" at 6 and then deleting 6..7 through one extracted `&mut Buffer`;
  - a transaction that replaces 6..7 with "X" and then with "Y", once it is undone or redone.
- **Quadratic work.** `splice` copies the whole piece list for every batch, once in `Known::new` and again in
  `record`. 10,000 one-byte appends recorded together take 4–8.5 s in a debug build.

## Change

Capture each batch's structure from the real texts when the batch is applied. Delete the reconstruction path.

1. crates/fe-buffer/src/buffer.rs:
   - `BufferEvent::Edited` gains `text: Text`, the text right after that batch. Cloning the rope is O(1).
   - `Buffer::edit` (which `edit_from` also goes through) emits `self.snapshot.text.clone()`.
   - `revert` also returns the text after each batch it produces, for example as
     `Vec<(AppliedBatch, Text)>`. These texts are not stored in the undo history. `undo` and `redo` emit each
     batch with its own text.
   - Update the exhaustive pattern in crates/fe-buffer/src/tests/mod.rs `events_describe_each_batch` by adding
     `..` and nothing else.
2. crates/fe-editor/src/lib.rs:
   - `History` owns its own `Subscription`, taken in `EditorModel::new` with `buffer.subscribe()`, and keeps
     `last: Text`.
   - `History::record` drains that subscription. For each `Edited { to, edits, text, .. }` it pushes
     `Recorded { version: to, edits, shapes }`, with `shapes[i] = Shape::of(edit, &last, &text)` computed
     from the real texts. Then it sets `last = text`. That is O(edits · log n) per batch, so the work is
     linear in batches.
   - Delete `Known`, `Pieced`, `Piece`, `Source` and `splice`. `Shape` keeps plain fields:
     `old_breaks: u32`, `new_breaks: u32`, `old_row_starts`/`new_row_starts: [bool; 2]`, and
     `last_row`/`last_row_before_end: Option<usize>`.
   - Change `FoldSpan::follow` to use those fields. Keep its fold rules (the same decisions as syncing after
     each batch). Keep `shapes: None`, which unfolds, only for a batch with no record.
   - `BufferMut::deref_mut` stops calling `record`. Remove the caveat about extracted `&mut Buffer` from the
     `buffer_mut` doc.
   - `catch_up` and `split` call `record()`, which now takes no snapshot. Update the docs on `History`,
     `catch_up` and `surviving_folds`.
   - Keep the names `EditorModel::history` and `History::batches` (with `len`/`is_empty`), because
     `shared_buffer.rs` reads them.
3. crates/fe-editor/src/tests/mod.rs: add `mod batch_structure;`. New tests go in the new
   crates/fe-editor/src/tests/batch_structure.rs and use only the base API: `parse`, `split`, `fold_rows`,
   `buffer_mut`, `start_transaction`/`end_transaction`, `undo`/`redo`, `display().folds()`. They must compile
   at base, where the regression tests must fail.

A draft of all five tests, compiled and run at base (the three regression tests fail there), is at
`~/.local/state/regatta/lead-fastedit/t94/batch_structure.rs`. Start from it.

## Out of scope

- fe-display (`DisplayMap::sync`/`fold_many`) and changing which edits keep a fold.
- Undo/redo semantics, and handling of `compact_log`.
- Changes to crates/fe-editor/src/tests/shared_buffer.rs.
- Any counter or perf API added only so tests can see it.
