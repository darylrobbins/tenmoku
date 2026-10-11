---
task_uid: "01m4pdr0afpps477nk2e5pq83a"
task: "T57"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T52

Findings of the review of T52 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-lsp/src/tests/decode_cost.rs:139: The references cases release the fake clock after confirming only one reply timer is armed. Request records are written before their answer futures register their sleeps, so observing both requests and sleeping for 50 ms does not guarantee both timers exist. If the second future is scheduled after advance(HOLD), it registers a new one-second delay that is never advanced; the navigation worker then waits indefinitely and the benchmark fails at recv_timeout.
  fix: Wait deterministically until all replies for the current run have registered their HOLD sleeps before starting the timer and advancing the clock, replacing the fixed 50 ms sleep with a bounded synchronization check.
  instances: crates/fe-lsp/src/tests/decode_cost.rs:139, crates/fe-lsp/src/tests/decode_cost.rs:141
