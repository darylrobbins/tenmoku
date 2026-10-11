---
task_uid: "01m4mad988n6fhr8he04wx76xk"
task: "T31"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The test's leak assertion (found == None) is unchanged, and it still requires more than 100 swaps and more than 10 rounds"
    required: true
    manual: false
  - id: "C2"
    text: "The walk loop runs until it reaches those minimums (or a deadline of at least 30 s), not for a fixed 1 s"
    required: true
    manual: false
  - id: "C3"
    text: "3 consecutive runs of the test (nextest -E 'test(=tests::finder_walk_safety::a_directory_swapped_for_a_symlink_mid_walk_is_not_followed)') pass, with each run's 1-min load average logged in the finish notes"
    required: true
    manual: false
  - id: "C4"
    text: "Of the worker's own changes, only crates/fe-project/src/tests/finder_walk_safety.rs changes; Regatta's spec file in docs/specs/ for this task is not the worker's change"
    required: true
    manual: false
---

# Flaky under load: fe-project finder_walk_safety::a_directory_swapped_for_a_symlink_mid_walk_is_not_followed: walk until enough rounds, not a fixed 1 s

## Problem

`fe-project tests::finder_walk_safety::a_directory_swapped_for_a_symlink_mid_walk_is_not_followed`
(`crates/fe-project/src/tests/finder_walk_safety.rs` ~274-324) failed in T22's gate (run
01m4m9wz71aeqxbjfr466ptsm3, base e5145c9, 2026-10-11 ~01:49Z) at line 323: `47301 swaps, 9 rounds`. It found
no leak: the safety assertion `found == None` held. What failed is the test's sanity bound on how much it
exercised, `swaps > 100 && walks > 10`. The walk loop stops after a fixed 1 s, and under gate load it managed
only 9 rounds.

## Fix

Change only the test. Run the loop until it reaches enough rounds, not until a fixed 1 s passes: for
example, keep going while `walks <= 10 || swaps <= 100` (or for at least 1 s, whichever is later), with a much
longer safety deadline (say 30 s) whose expiry fails with the counts. Keep the leak assertion as it is.
