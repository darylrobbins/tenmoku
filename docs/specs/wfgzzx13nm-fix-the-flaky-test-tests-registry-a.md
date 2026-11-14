---
task_uid: "01m7djfkff2gh6zewfgzzx13nm"
task: "T652"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::registry::a_fixed_command_brings_back_the_documents_of_a_failed_server passes 20 consecutive runs under load alongside the integration suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `tests::registry::a_fixed_command_brings_back_the_documents_of_a_failed_server`, which passed only on reruns

fe-lsp.tests::registry::a_fixed_command_brings_back_the_documents_of_a_failed_server failed in a gate and passed only on its reruns.

- Gate run: 01m7dhttqvwsms87sfrm5cgm95
- Candidate: 01m7dg15sx6v0mcr9s5zc48b9x
- Task: T633
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

Fix the test, not the code under it. Prove the fix with repeated runs under load: `go test -count=20` on the test alongside the integration suite. Record the evidence in the task's decision file (docs/decisions/<task_key>-<slug>.md).
