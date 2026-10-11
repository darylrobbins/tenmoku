---
task_uid: "01m4kxpvmrjnyxyg88nbcwv0p1"
task: "T15"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T252 (Mac T280)

Findings of the review of T252 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-buffer/src/buffer.rs:476: Suspending the caller's transaction removes its protection from trim_history. If acceptance uses the same origin and exceeds the history limit, record treats the suspended transaction as closed and deletes it; the restoration check then silently leaves it closed. For example, set_history_limit(0), open an Ai transaction, make an unrelated edit, then accept a hunk: subsequent caller edits receive a new transaction ID, end_transaction returns None, and the earlier caller edit is no longer undoable. Both acceptance methods violate the requirement to preserve the caller-owned transaction.
  fix: Keep suspended transactions protected from history eviction while recording the independent acceptance transaction, and restore the caller's ID and nesting depth. Add public-API regression tests for both acceptance methods with matching origins and a history limit that acceptance exceeds, checking subsequent edits retain the caller's ID and its transaction remains undoable.
  instances: crates/fe-buffer/src/buffer.rs:476, crates/fe-buffer/src/proposed.rs:274, crates/fe-buffer/src/proposed.rs:312



Moved from the Mac board: Mac T280. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
