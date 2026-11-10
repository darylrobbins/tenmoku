---
task_uid: "01m73rzmtds6hva0y72w5qyet0"
task: "T568"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "private_key_is_read_from_stdin_only passes 20 consecutive runs under load alongside the integration suite, with the evidence in the decision file"
    required: true
    manual: false
  - id: "C2"
    text: "The fix changes only test code, or the task is retyped to a bugfix with a regression test"
    required: true
    manual: false
---

# Fix the flaky test `private_key_is_read_from_stdin_only`, which passed only on reruns

fe-registry::signing.private_key_is_read_from_stdin_only failed in a gate and passed only on its reruns.

- Gate run: 01m73rqra1f6wra322rh1hc21v
- Candidate: 01m73qv5jssk0zga0xfvcbsw21
- Task: T552 (unrelated to the failure; helium load ~22-37)
- Rerun results:
  - rerun 1: passed
  - rerun 2: passed
  - rerun 3: passed

## Likely cause

The failure was a BrokenPipe panic at crates/fe-registry/tests/signing.rs:90: the test helper `run` does
`child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap()`. When the binary exits (or closes stdin)
before reading all of stdin, e.g. a case that refuses early, the write fails with EPIPE under load. The helper
should tolerate `ErrorKind::BrokenPipe` on that write (and drop stdin before waiting), then let the test assert
on the child's exit status and output as before. Check every caller of `run` still asserts what it did.

Fix the test, not the code under it. Prove the fix with repeated runs under load: run
`cargo test -p fe-registry --test signing private_key_is_read_from_stdin_only` 20 times while the rest of the
fe-registry test suite runs alongside. Record the evidence in the task's decision file
(docs/decisions/<task_key>-<slug>.md).
