---
task_uid: "01m7c79f0n4hka8vncw33mvh9a"
task: "T641"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::add_ons::first_answer::primary_non_empty_answer_wins_even_when_add_on_is_first passes 20 consecutive runs under load alongside the integration suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `tests::add_ons::first_answer::primary_non_empty_answer_wins_even_when_add_on_is_first`, which passed only on reruns

fe-lsp.tests::add_ons::first_answer::primary_non_empty_answer_wins_even_when_add_on_is_first failed in a gate and passed only on its reruns.

- Gate run: 01m7c6hr79dbpt9t97apwvcf7r
- Candidate: 01m7c4966f1bfm9faa1pgf8cjg
- Task: T629
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).
