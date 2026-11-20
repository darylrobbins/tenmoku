---
task_uid: "01m7wxe3f0eahtzyx8ddynzbhe"
task: "T719"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "plugin_bridge::plugin_lsp::plugin_lsp_tests::a_notification_to_a_stopped_session_is_not_found passes 20 consecutive runs while `cargo test -p fe-workbench` runs alongside as load (this repo has no separate integration suite), with the commands and results in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `plugin_bridge::plugin_lsp::plugin_lsp_tests::a_notification_to_a_stopped_session_is_not_found`, which passed only on reruns

fe-workbench.plugin_bridge::plugin_lsp::plugin_lsp_tests::a_notification_to_a_stopped_session_is_not_found failed in a gate and passed only on its reruns.

- Gate run: 01m7ww8mbgs9n869dwk1zd0bxg
- Candidate: 01m7wv0ybc00zmzg42x8gcbs26
- Task: T715
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).
