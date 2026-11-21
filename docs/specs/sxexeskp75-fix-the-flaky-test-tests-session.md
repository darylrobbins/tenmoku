---
task_uid: "01m7y16a4ete43k8sxexeskp75"
task: "T732"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::session_uncertain::a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends passes 20 consecutive runs under load alongside the integration suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `tests::session_uncertain::a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends`, which passed only on reruns

fe-dap.tests::session_uncertain::a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends failed in a gate and passed only on its reruns.

- Gate run: 01m7y0prbfe07c2cpbafz4c21e
- Candidate: 01m7xyssj3mynsdbm2qa0h9dzm
- Task: T712
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).


## Lead note (helium, 2026-11-21)

T727 (d101d2076, landed 01:25Z) already changed this test's timing in crates/fe-dap/src/tests/session_uncertain.rs. This failure ran on base 326a89848, which contains T727's fix, so a second test-only tweak is not enough on its face. Read T727's change and its spec/decision first (docs/specs/yzpxm4zpeh-*, docs/decisions/yzpxm4zpeh-*), then find out whether the session code under the test has a real race (a delayed failure arriving after stop). If it does, fix the code and retype the task to a bugfix with a regression test (C2). If it is still the test, explain why T727's fix missed it.

This is a Rust repo: prove the fix with repeated runs under load via nextest (e.g. `cargo nextest run -p fe-dap -E 'test(=tests::session_uncertain::a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends)'` in a loop of 20, alongside other load), not `go test`. Ask the lead before any cargo build and before rgt finish.
