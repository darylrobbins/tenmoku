---
task_uid: "01m4s0qv4k5gfnw6psa6fmwm8m"
task: "T91"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "an_edit_from_an_ended_session passes 30/30 consecutive nextest runs under load (load average logged), and still asserts the ended-session answer, an empty queue and an unchanged file"
    required: true
    manual: false
  - id: "C2"
    text: "No other workspace_edit test fails in those runs"
    required: true
    manual: false
---

# Make fe-workbench's an_edit_from_an_ended_session test deterministic: the fake's 10 s applyEdit timeout races the test's own 10 s wait under load

`fe-workbench`'s test `workspace_edit_tests.rs::an_edit_from_an_ended_session` (added by T83, Mac T472) is flaky under load. It failed twice on neon on 2026-10-12, both times at about 10 s: T87's gate (run 01m4rxcftqfzsrcpwcrkx8mx7t, load ~30) and T84's gate (run 01m4rzq77mcs0es6c3ja5w84ty, load ~16–20).

The failure: the helper `push_edit` sends `workspace/applyEdit` from the fake server on its own thread, and the fake's request gives up after 10 s ("no answer to workspace/applyEdit in 10s", workspace_edit_tests.rs:80). The test then waits up to another 10 s for the edit to reach the workbench (`lsp_queued() > 0`) before calling `lsp_stop`, whose answer ends the request. Under load those two windows overlap: by the time the stop answers, the fake has already timed out, so `asked.join()` panics at line 330.

Make it deterministic without weakening what it checks: an edit queued when the server's session ends is answered `{"applied": false, "failureReason": "the server's session ended"}`, is never applied, and leaves the queue empty and the file unchanged. For example, give this request a fake-side timeout well beyond the test's own wait (if the fake supports a per-request timeout), or restructure the wait so it can't race the fake's clock. Check whether other tests in workspace_edit_tests.rs that use `push_edit` share the hazard, and fix them the same way.

Verify with repeated runs under load: run the test (and the file) at least 30 times with `cargo nextest run -p fe-workbench --profile ci` while the machine is busy (e.g. a parallel workspace build), log the load average, and record the counts in the commit.
