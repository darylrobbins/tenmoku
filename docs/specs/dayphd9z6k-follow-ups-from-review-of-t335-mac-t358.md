---
task_uid: "01m4nxsnabtcr944dayphd9z6k"
task: "T41"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T335 (Mac T358)

Findings of the review of T335 that don't block the merge (below merge.block_severity):

- F1 low kiln/crates/kiln-core/src/tests/watch_reconcile.rs:87: F7 remains unresolved: sync_channel(1024) and a fast producer do not guarantee a continuously queued receiver. The consumer can drain the queue while the producer is descheduled, allowing the old starvation-prone implementation to reconcile during a gap and pass. The test also never asserts that the change is reported before the backlog empties.
  fix: Use a deterministic event-source or scheduling hook to keep events available across the reconciliation deadline, and assert that the silently changed file is reported while unread events remain. Verify that removing the deadline checks makes this regression test fail.
  instances: kiln/crates/kiln-core/src/tests/watch_reconcile.rs:87, kiln/crates/kiln-core/src/tests/watch_reconcile.rs:107, kiln/crates/kiln-core/src/tests/watch_reconcile.rs:130
