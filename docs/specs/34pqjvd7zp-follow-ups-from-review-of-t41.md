---
task_uid: "01m4p01m56ncsy0d34pqjvd7zp"
task: "T46"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T41

Findings of the review of T41 that don't block the merge (below merge.block_severity):

- F1 medium kiln/crates/kiln-core/src/tests/watch_reconcile.rs:111: The injected event sender outlives the watcher, so teardown cannot disconnect the receiver. If the worker drains the remaining backlog before the test thread drops the watcher, it enters real recv_timeout waits while fake time advances only 30 ms per reading. The watcher checks whether it was dropped only at the next fake deadline, so joining it can stall for roughly 150 seconds, including during assertion failures. This introduces scheduling-dependent test delays despite TIMEOUT being 30 seconds.
  fix: Arrange for the injected sender to drop before the watcher on both normal completion and unwinding, while keeping it alive throughout the backlog assertions; for example, move it into a binding declared after the watcher.
  instances: kiln/crates/kiln-core/src/tests/watch_reconcile.rs:111
