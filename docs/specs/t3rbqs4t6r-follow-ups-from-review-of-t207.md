---
task_uid: "01m4exsd7kged7r3t3rbqs4t6r"
task: "T232"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T207

Findings of the review of T207 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/history.rs:91: Original carets are still lost when the second insert outlives the measuring callback. Start `A` and type `y` inside `closed_by_a_clone`, but dispatch its normal_mode after the helper returns. The outer completion publishes transaction=None because the nested transaction remains open. Ending the second insert does not adopt that completion and records its newer carets. On subsequent undo, settle receives kept=None and has no pending record to update, so it discards the original carets; undo/redo restore the second insert's position. The added regression closes both sessions inside the callback and misses this ordering. The same branch also fails when a joined normal edit already created an entries record and record_pending therefore declines to create a pending record.
  fix: Preserve the earliest caret record by transaction ID independently of whether the completion returned an ID or a matching pending record still exists, reconciling already-recorded entries as well. Add regressions that leave the second insert open across publication, then close it before adoption and verify undo/redo text and original caret positions, including an intervening normal edit.
  instances: crates/fe-vim/src/history.rs:91
