---
task_uid: "01m7xn35ymf81ed9yzpxm4zpeh"
task: "T727"
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

- Gate run: 01m7xmexnm016tyxqqk0wps1hz
- Candidate: 01m7xjzfxxkv90w064cdqyv7nz
- Task: T721
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).
