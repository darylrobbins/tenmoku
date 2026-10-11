---
task_uid: "01m4pgsdpqshyskxdhv3jdt8ey"
task: "T58"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T56

Findings of the review of T56 that don't block the merge (below merge.block_severity):

- F7 medium crates/fe-workbench/src/config_problems.rs:256: Removing the shown_serial guard allows a pending pick to overwrite later deliberate tab navigation. Clicking another existing tab, cycling tabs, closing the active tab, or reordering tabs does not increment open_serial. If one of those actions happens during the read, this guard still passes and the diagnostic file takes focus, undoing the user's newer choice.
  fix: Track deliberate navigation intents separately from asynchronous display changes, including tab activation, cycling, closing and reordering. Reject a pending pick after a newer intent while still allowing older read completions. Add deterministic regressions holding the pick's read across these tab actions.
  instances: crates/fe-workbench/src/config_problems.rs:256, crates/fe-workbench/src/config_problems.rs:391
