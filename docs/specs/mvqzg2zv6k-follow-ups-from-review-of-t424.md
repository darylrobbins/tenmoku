---
task_uid: "01m6pw51fzxgmmbdmvqzg2zv6k"
task: "T458"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Follow-ups from review of T424

Findings of the review of T424 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-05):

- F3 medium crates/fe-workbench/src/task_defs.rs:739: The archive predicate also rejects `a()`, which GNU make treats as a literal target with an empty archive-member automatic variable. This valid target remains absent from detected tasks. The predicate is unchanged since the previous review, so this is a nonblocking follow-up.
  fix: Require nonempty member text when identifying archive syntax, and add a regression asserting that `a():` produces the task `make a()`.
  instances: crates/fe-workbench/src/task_defs.rs:739
