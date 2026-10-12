---
task_uid: "01m4pvwnxqgqvjtkefv4adfw88"
task: "T448"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T414

Findings of the review of T414 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/tests/feature_stubs.rs:65: Every no-change check runs on the empty second row: `xjl` lands there, and `vl` cannot extend the visual cursor. Incorrectly delegating a new motion to existing Left or Right would therefore pass the motion assertions. This weakens the primary regression test for the new dispatch plumbing.
  fix: Run the checks from an interior position on a populated row and create a visual selection with distinct anchor and cursor positions; assert those fixture preconditions. Retain the empty-row checks as additional coverage.
  instances: crates/fe-vim/src/tests/feature_stubs.rs:65, crates/fe-vim/src/tests/feature_stubs.rs:67
