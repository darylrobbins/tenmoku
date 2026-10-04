---
task_uid: "01m41jh16153t7q8dysfd0jzxv"
task: "T38"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T21

Findings of the review of T21 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-syntax/src/syntax/reparse_budget_tests.rs:71: The accepted incremental result records its real elapsed time, replacing the fabricated BUDGET / 2 timing. If this parse takes more than 1 ms, including scheduler delays on a loaded runner, the final assertion fails even when stale-result rejection works correctly. Disabling the final parse's timeout does not disable its historical-budget check.
  fix: Store the accepted parse result in a variable, set its elapsed to BUDGET / 2, and then finish it before delivering the stale result.
  instances: crates/fe-syntax/src/syntax/reparse_budget_tests.rs:71
