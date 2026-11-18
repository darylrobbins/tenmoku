---
task_uid: "01m7pbjgfhhrt8hxjfjempqa4y"
task: "T690"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::build_timeline_view::a_running_build_grows_its_bars passes 20 consecutive runs under load alongside the integration suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `tests::build_timeline_view::a_running_build_grows_its_bars`, which passed only on reruns

fe-workbench.tests::build_timeline_view::a_running_build_grows_its_bars failed in a gate and passed only on its reruns.

- Gate run: 01m7pazskprwa8wpbjs8x6d47f
- Candidate: 01m7p9w6dj8a7gt3bm7vkdhjcq
- Task: T682
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).
