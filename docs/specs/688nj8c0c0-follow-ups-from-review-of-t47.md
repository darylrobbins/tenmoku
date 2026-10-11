---
task_uid: "01m4pbzs1xzfpw3n688nj8c0c0"
task: "T55"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T47

Findings of the review of T47 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-lsp/src/tests/pull_diagnostics.rs:635: The reply predicate still searches across all launches without capturing the raced request's launch. This test previously restarts the server, and async-lsp resets request IDs for each session, so an earlier launch's reply with the same ID can satisfy the wait before the raced reply arrives. This preserves the false-positive mechanism that T47 was intended to remove.
  fix: Capture the current launch together with the request ID, and use records_of and wait_for_at for that launch when identifying and waiting for the raced pull.
  instances: crates/fe-lsp/src/tests/pull_diagnostics.rs:611, crates/fe-lsp/src/tests/pull_diagnostics.rs:635

- F2 medium crates/fe-lsp/src/tests/pull_diagnostics.rs:634: Even with launch filtering, Record::Replied does not establish reply delivery: fake_router records it before returning the response to async-lsp for serialization and transmission. Likewise, notify only queues the publication. Unblocking client writes immediately after this predicate can therefore let the actor process Close/Open before either diagnostic reaches it. The pull regression and new push coverage can pass with their active-identity guards removed.
  fix: Add synchronization proving that both the publication and the specific pull reply are available to the client before releasing the blocked actor, or explicitly hold lifecycle processing until both diagnostic paths have run. Verify that removing each guard independently makes its regression fail reliably.
  instances: crates/fe-lsp/src/tests/pull_diagnostics.rs:628, crates/fe-lsp/src/tests/pull_diagnostics.rs:634

## Implementation notes

- F1: the raced pull's id is taken from, and its reply awaited in, the current launch's records (`records_of`, `wait_for_at`).
- F2: the fake records each message it queues for the client (`FakeServer::wait_sent`); the test waits for the raced reply and the publication to be on the wire before reopening client writes. Removing either active-identity guard in `document.rs` (`pushed`, `pulled`) makes `pushed_and_pulled_merge` fail 3/3.
