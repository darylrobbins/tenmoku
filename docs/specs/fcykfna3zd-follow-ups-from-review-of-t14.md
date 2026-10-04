---
task_uid: "01m41t7b44g9m1hmfcykfna3zd"
task: "T39"
rev: 1
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T14

Findings of the review of T14 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/src/log.rs:213: The floor check rejects only the directly reverted batch, ignoring collapse roots and inversion links below the floor. Compaction also retains undone entries whose original batches were removed. The new test demonstrates that inserting, undoing, compacting past the original batch, and redoing changes text successfully, contrary to the supplied spec's conservative refusal and history-pruning requirements. The implementation notes explicitly document this departure.
  fix: Apply the specified floor policy to collapse and inversion dependencies, prune closed history entries whose batches are below the floor, and update the redo assertions and implementation notes accordingly.
  instances: crates/fe-buffer/src/log.rs:213, crates/fe-buffer/src/buffer.rs:287, crates/fe-buffer/src/tests/log_compaction.rs:140, docs/specs/x36abp42s7-compact-edit-log-entries-below-the.md:87, docs/specs/x36abp42s7-compact-edit-log-entries-below-the.md:92

## Implementation notes

F1 is fixed by applying the T14 spec's floor policy as written:

- `EditLog::map_for_history` returns `None` (so undo or redo fails with
  `UndoError::Conflict` and leaves the text unchanged) when any collapse root
  the boundary depends on is below the floor: the root of the batch being
  reverted, of an entry mapped through, or of an inherited collapse. Roots
  are never above `inverts`, so inversion links below the floor are covered.
- `Buffer::compact_log` drops a closed history entry when any of its batches,
  applied or undone, is below the floor. Undo, compact past the original
  batch, then redo now finds nothing to redo.
- Tests: the redo scenario in
  `undo_below_the_floor_is_refused_without_changing_text` now expects
  `Ok(None)`, and `undo_that_depends_on_an_edit_below_the_floor_is_refused`
  covers a redo of a compacted edit and a transaction mapped through an undo
  of one. The T14 spec's implementation notes are updated to match.
