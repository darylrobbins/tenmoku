---
task_uid: "01m4kxpvrkcd9nxsy74b1dqhvf"
task: "T19"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T269 (Mac T297)

Findings of the review of T269 that don't block the merge (below merge.block_severity):

- F1 low crates/fe-fileview/src/index.rs:156: An empty build_stepped initializes Complete and creates no worker, but leaves the gate's exited flag false. Consequently Step::exited_within always waits out its timeout and returns false, even after dropping the index, falsely reporting an outstanding worker. The empty-file test uses build, so it misses this completion edge case.
  fix: Mark the gate exited when startup skips the worker for an empty file. Add a stepped empty-file test checking that next returns None and exited_within returns true, including after dropping the index.
  instances: crates/fe-fileview/src/index.rs:156



Moved from the Mac board: Mac T297. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
