---
task_uid: "01m4kwwx9y6bze17tkx9ccrq93"
task: "T335"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T334

Findings of the review of T334 that don't block the merge (below merge.block_severity):

- F5 medium crates/fe-project/src/watcher.rs:177: Raw-path counting now includes unflagged Access events, which Batch intentionally ignores. notify's Linux backend reports file and directory opens, so read-only searches can arm recovery. In a project with at least 1,000 directories, the rescan's own directory opens can re-arm recovery after its final rescan, producing repeated full walks without any writes.
  fix: Exclude unflagged Access events from burst counting while retaining paths from rescan-flagged events, and add synthetic coverage showing that read-only access bursts do not schedule rescans.
  instances: crates/fe-project/src/watcher.rs:177

- F6 medium crates/fe-config/src/watch.rs:107: The next deadline is assigned before reconciliation runs, and disconnection is checked only in the receive branch. If each reconciliation takes at least RECONCILE, such as with config chains on a slow filesystem, every subsequent iteration immediately reconciles again. Dropping ConfigWatcher disconnects the receiver but its join never completes because the thread never checks it.
  fix: Ensure shutdown/disconnection is checked independently of reconciliation deadlines, and prevent slow reconciliation from permanently bypassing receives while preserving protection against event starvation.
  instances: crates/fe-config/src/watch.rs:106, crates/fe-config/src/watch.rs:107, crates/fe-config/src/watch.rs:123

- F7 low crates/fe-config/src/tests/watch_reconcile.rs:124: The new noise test leaves 10 ms gaps between events, so the previous deadline-based implementation can reconcile during those gaps and pass. It does not reliably exercise F1's continuously queued receiver or unbounded drain.
  fix: Add deterministic coverage that keeps events queued across the reconciliation deadline and verifies that a silently changed tracked file is reported before the backlog empties.
  instances: crates/fe-config/src/tests/watch_reconcile.rs:124


Paths (kiln K1.2 move): crates/fe-config/src/watch.rs is now kiln/crates/kiln-core/src/watch.rs (F6: the reconcile loop around lines 106-123) and crates/fe-config/src/tests/watch_reconcile.rs is now kiln/crates/kiln-core/src/tests/watch_reconcile.rs (F7: the Noise helper, around line 120), declared in kiln/crates/kiln-core/src/tests/mod.rs.
