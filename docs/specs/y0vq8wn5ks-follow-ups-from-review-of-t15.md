---
task_uid: "01m4kzhz7d8d6qfcy0vq8wn5ks"
task: "T25"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T15

Findings of the review of T15 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/tests/proposed_change.rs:366: The regression test never verifies that the caller's transaction remains undoable, as the spec explicitly requests. Closing it with the zero history limit evicts it, and the only undo assertion checks the acceptance transaction. Returning the caller's ID from end_transaction does not prove that its original and subsequent edits were preserved together.
  fix: For both acceptance methods, assert the subsequent edit returns Some(outer), raise the history limit before closing the caller transaction, then undo the acceptance and the caller transaction and verify their respective text changes. Include a nested caller transaction to verify nesting depth is preserved.
  instances: crates/fe-buffer/tests/proposed_change.rs:366
