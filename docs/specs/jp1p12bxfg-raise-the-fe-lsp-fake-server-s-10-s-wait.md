---
task_uid: "01m4s0c20shqaz49jp1p12bxfg"
task: "T59"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The fake's WAIT bound is 60 s; no test depends on WAIT expiring or asserts a duration under it"
    required: true
    manual: false
  - id: "C2"
    text: "Test-only or test-support only: no production code changes; the affected crate's tests pass under nextest"
    required: true
    manual: false
---

# Raise the fe-lsp fake server's 10 s WAIT to 60 s, so loaded machines don't fail LSP tests like workspace_edit's an_edit_from_an_ended_session (flake)

Flake (twice on argon under load ~20-36, 2026-10-12): `fe-workbench workspace_edit::tests::an_edit_from_an_ended_session` failed with "no answer to workspace/applyEdit in 10s" (workspace_edit_tests.rs:80, then the join unwrap at :330). Runs: 01m4rz27bbw89ep0ngc3q2vhze (argon trunk sync 54d456b) and T54's gate. It took 10.4 s and passes on its own.

Cause: the fake language server in crates/fe-lsp/src/tests/fake.rs, `pub mod fake` and test support for every LSP-facing crate, bounds every wait with `const WAIT: Duration = Duration::from_secs(10)`. The bound exists to fail a test that would otherwise hang, not to measure speed. On a loaded machine an honest answer can take longer than 10 s.

Fix (test support only): raise WAIT to 60 s and keep its role and messages. Check that no test asserts that something takes less than WAIT, and that no test relies on WAIT expiring for its expected outcome; if one does, give that test its own short bound instead. No production code changes.

The test came from the Mac (neon T472 = Mac T472, via host stack 3). The fake is shared with the Mac, so this change goes back in the next argon stack.
