---
task_uid: "01m4fb63w1r1kvqgnw7rj1g24t"
task: "T252"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T240

Findings of the review of T240 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/src/proposed.rs:310: Acceptance does not isolate its transaction from an already open transaction. Calling accept_all after buffer.start_transaction(Origin::User) panics in debug builds because start_transaction forbids nesting another origin; Vim insert mode keeps such user transactions open. If the open transaction has the proposal's origin, both accept and accept_all instead join it, returning its ID and causing undo to revert unrelated edits or separately accepted hunks together. The new tests exercise only buffers without an open transaction.
  fix: Ensure both acceptance methods create an independent transaction while preserving any caller-owned open transaction. Add public-API integration tests with open User and matching Ai transactions, verifying no panic, independent transaction IDs, and undo that reverts only the accepted hunk or accept_all batch.
  instances: crates/fe-buffer/src/proposed.rs:274, crates/fe-buffer/src/proposed.rs:310
