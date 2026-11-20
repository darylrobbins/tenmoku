---
task_uid: "01m7wn3wx6w9g6rm0gvkqahwrg"
task: "T718"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cancelling_a_load_drops_a_request_waiting_on_a_slow_server waits (bounded) for the $/cancelRequest record instead of reading the records once; other one-shot reads after an async send in plugin_lsp_tests.rs are fixed the same way"
    required: true
    manual: false
  - id: "C2"
    text: "The test passes 20 runs in a row under load (e.g. alongside a parallel cargo build), shown in the finish note"
    required: true
    manual: false
---

# Flake: fe-workbench plugin_lsp_tests cancelling_a_load_drops_a_request_waiting_on_a_slow_server reads the fake server's records once before the async $/cancelRequest arrives (Mac T2035)

Flake seen in T1702's finish gate (run 01m7wjsmzmgd4naa1wp1hy9w5n, 11-20, load ~20 with two finishes): 5756/5757 passed; only this test failed, at plugin_lsp_tests.rs:813 "the server's request was cancelled". Six warm reruns passed (0.37-0.71 s, load 10-14).

Cause (read on main 436039213): after the cancelled call returns, the test reads `s.fake.records()` once for a `$/cancelRequest` notification. The cancel is sent asynchronously to the fake server, so under load the call can finish before the fake has recorded the notification.

Fix (test only): poll the fake's records for the `$/cancelRequest` notification until a deadline (the existing `until`, drawing between polls like the loops above), then assert. Check the neighbouring tests in the same file for the same one-shot read after an async send (e.g. a_notification_of_a_cancelled_load_is_not_sent asserts an absence: leave absence checks as they are, but make sure each waits on a positive signal first) and fix any that race the same way. No production change.

## Helium filing note

- Lent from the Mac as y154. Test-only flake fix: no production change and no regression test.
- The file arrives with mac/main 436039213 (helium sync pending); the lead readies this card after that sync.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp. For C2, run the test 20 times in a row
  alongside a parallel cargo build, and paste the counts and load average into the finish note.
- Criteria tests need full cargo test paths.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if a criterion can't be met as written.
  Run rgt finish once, and wait for its result before you report.
